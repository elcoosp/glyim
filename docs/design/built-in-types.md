That's a genuinely interesting idea. Let me lay out the design space because there are some non-obvious decisions and edge cases you'll hit.

## The Core Idea

```
struct User {
    name: String
    age: Int
    email: String

    fn greet()  { print("Hi " + name) }
    fn grow()   { age += 1 }
    fn contact() { send(email) }
    fn summary() { print(name + ", age " + age) }
}

type LiteUser = Omit<User, age>
```

This would yield:

```
struct LiteUser {
    name: String
    email: String

    fn greet()   // kept: no dependency on age
    fn contact() // kept: no dependency on age
    // grow()    REMOVED: directly uses age
    // summary() REMOVED: references age
}
```

The field removal is trivial. The method pruning is the interesting part — it's essentially **dependency analysis on the struct's own methods**.

---

## Design Decisions You Need to Make

### 1. Transitivity through method calls

```
struct User {
    age: Int
    fn grow()      { age += 1 }
    fn celebrate() { grow(); party() }
}
```

Does `Omit<User, age>` also remove `celebrate()` because it *indirectly* depends on `age` through `grow()`?

**Yes, it should.** Otherwise you get a method that calls a removed method — a compile error. You need to compute the **transitive closure** of field dependencies.

### 2. What about methods that *could* work without the field?

```
struct User {
    age: Int
    fn describe() {
        if has(age) {
            print(age)
        } else {
            print("age unknown")
        }
    }
}
```

Three options:

| Strategy | Behavior |
|---|---|
| **Strict** | Remove if the field is *referenced anywhere* in the body |
| **Lenient** | Keep if the field access is guarded / optional |
| **Explicit** | Only keep methods annotated `#[survives_omit(age)]` |

I'd recommend **strict by default, explicit override**. It's predictable and the annotation lets programmers express intent when they know better than the analyzer.

### 3. `self` inference — what counts as "uses the field"?

```
fn foo(other: User) { print(other.age) }
```

Does this method on `User` use `self.age`? No — it reads `age` from a different instance. You need to distinguish **`self.age`** (or implicit `this.age`) from accessing the field on other values. This matters.

### 4. Methods that return the omitted type

```
struct User {
    age: Int
    fn clone_age() -> Int { age }
}
```

Should this be removed? The method doesn't *mutate* `age`, it just reads it. But without `age`, the method body is invalid. So yes, strict dependency says remove it.

But consider:

```
fn default_age() -> Int { 25 }
```

This doesn't reference `age` at all — it should survive.

### 5. Re-adding methods after omission

Users will sometimes want to omit a field but **substitute** a different implementation:

```
type LiteUser = Omit<User, age> with {
    fn summary() { print(name + ", age hidden") }
}
```

This is your escape hatch. The language should support it, or people will fight the type system.

---

## Suggested Algorithm

Given `Omit<T, fields F>`:

```
1. Remove fields F from T

2. Build a dependency graph:
   - For each method M on T, compute:
     direct_fields(M) = fields of T referenced via self/this in M's body
     direct_methods(M) = other methods on T called by M

3. Mark fields in F as "dead"

4. Propagate:
   - A method is "dead" if any field in direct_fields(M) is dead
   - A method is "dead" if any method in direct_methods(M) is dead
   - Repeat until fixed point (transitive closure)

5. Remove all dead methods

6. Any method with #[survives_omit(...)] overrides step 4
   (but the method body must still type-check in the omitted struct)
```

---

## Implementation Considerations

**Type-checking the result:** The resulting `Omit` type is a *new* struct. You need to ensure that all surviving methods type-check against the reduced field set. Strict removal guarantees this, but with `#[survives_omit]` or the `with { ... }` syntax, you need to re-check.

**Method dispatch on the base type:** If you have `fn process(u: User)`, can you pass a `LiteUser`? Only if `LiteUser <: User` — which it *isn't* in general, because `LiteUser` lacks methods. You might want:

```
fn process(u: Omit<User, age>)  // accepts both
```

This makes `Omit` a **supertype** (fewer fields, fewer methods = more general). That's the natural direction for subtyping.

**Partial omit for methods only?** Someone will ask: "can I omit just the `grow` method but keep `age`?" That's a different feature (`OmitMethods`?), but worth knowing it's a separate concern.

---

## A Cleaner Syntax Idea

Rather than just a type-level operator, make it structural:

```
type LiteUser = User - { age }
// or
type LiteUser = User omitting { age }
```

The `-` sigil mirrors set difference and reads naturally. For the override syntax:

```
type LiteUser = User - { age } overriding {
    fn summary() { print(name + ", age hidden") }
}
```

---

## TL;DR — The Idea Is Sound, The Devil Is In

1. **Transitive** dependency propagation (must do)
2. **Strict vs. lenient** field reference analysis (recommend strict + annotation)
3. **`self` vs. other-instance** field access distinction (must do)
4. **Override/substitution** syntax (highly recommended)
5. **Subtyping direction** — `Omit<T, K>` is a *supertype*, not a subtype

This is a real ergonomic win over TypeScript's `Omit` which only handles the field layer and silently leaves you with methods that can't work. Want to dig into any of these aspects?
Since your language is Rust-like (nominal typing, traits, ownership) but includes **compile-time execution (CTE), reflection, and macros**, you have a massive superpower. TypeScript's utility types work because TS uses structural typing and erases everything at runtime. Your language can do TS one better: **type transformations that generate valid, memory-safe, compiled structs and traits at compile time.**

Here is a set of built-in type utilities tailored for a Rust-like language with CTE and reflection, categorized by their use case:

---

### 1. Structural Selectors (The `Omit` Family)

These create new nominal types by subsetting or combining existing ones.

*   **`Pick<T, Fields>`**: The inverse of `Omit`. Keeps only the specified fields and *only* the methods that depend exclusively on those fields.
    ```rust
    type Credentials = Pick<User, { email, password }>
    // Automatically drops methods like `greet()` that need `name`
    ```

*   **`Merge<A, B>` (or `Intersect<A, B>`)**: Combines two structs. If fields overlap, `B` wins (like TypeScript's `&` but with explicit override rules).
    ```rust
    struct Base { id: Int, fn print_id() { ... } }
    struct Patch { id: String, fn validate() { ... } }
    type Patched = Merge<Base, Patch> // id becomes String, has both methods
    ```
    *Rust-like twist:* The macro system needs to figure out trait implementations. If `Base` implements `Serialize` and `Patch` doesn't, does `Patched`? Yes, but only if the compiler can autogenerate the `Serialize` impl for the new struct layout.

---

### 2. Field Modifiers (The CTE Power-ups)

In TypeScript, `Partial<T>` just adds `?` to the type. In your language, modifying a field's type (e.g., wrapping it in an `Option`) normally breaks method bodies. **With CTE and reflection, you can auto-fix the methods.**

*   **`Partial<T>` / `Optionify<T, Fields>`**: Wraps specified fields in `Option<T>`. 
    *   *The Magic:* The compiler rewrites methods that access those fields to handle the `Option`. A method returning `T` becomes returning `Option<T>`. A method mutating `T` might only execute if the field is `Some`.
    ```rust
    type MaybeUser = Optionify<User, { age }>
    // summary() is auto-rewritten to: 
    // fn summary() { print(name + ", age " + age.unwrap_or("unknown")) }
    ```

*   **`Required<T, Fields>`**: The inverse. Unwraps `Option<T>` fields. Forces compile-time checks that the field is initialized upon construction.

*   **`Immutable<T, Fields>`**: Converts specific fields from `mut` to immutable. Automatically strips any `&mut self` methods that attempt to write to those fields.

*   **`MapFields<T, Trait>`**: Applies a type-level function to every field.
    ```rust
    // Wraps every field in an Arc<Mutex<T>>
    type ThreadSafeUser = MapFields<User, Arc<Mutex<_>>>
    // Auto-rewrites methods to acquire locks before accessing self.*
    ```

---

### 3. Signature Extractors

Using reflection, you can treat functions and traits as data structures at compile time.

*   **`Args<F>` / `Return<F>`**: Extracts the parameter types or return type of a function.
    ```rust
    fn process_user(u: User, flags: Int) -> Result<Bool, Error> {}
    
    type ProcessArgs = Args<typeof process_user> // Tuple: (User, Int)
    type ProcessResult = Return<typeof process_user> // Result<Bool, Error>
    ```

*   **`MethodSignature<T, method_name>`**: Extracts the full signature of a method on a struct, allowing you to generate proxy functions or wrappers dynamically.

---

### 4. Trait Transformers (The Macro Holy Grail)

This is where your language can completely one-up both Rust and TypeScript. Transforming structs is easy; transforming *behavior* (traits/impls) is hard.

*   **`ImplFor<T, Trait>`**: Generates a stub implementation of a Trait for a struct `T` using reflection. If the trait requires a `name: String` field and `T` has it, it auto-maps them.
    ```rust
    // Automatically implements the `Identifiable` trait for User
    // by mapping `Identifiable::id` to `User::email`
    impl Identifiable for User = ImplFor<User, Identifiable> with { map id -> email }
    ```

*   **`Delegate<T, Trait, field_name>`**: Auto-generates a Trait implementation for `T` by forwarding all calls to a specific field. (Rust uses `derive_more` for this, but as a built-in type-level operator, it's incredibly powerful).
    ```rust
    struct Wrapper { inner: User }
    // Auto-generates impl PartialEq for Wrapper by calling self.inner.partial_eq()
    type DelegatedWrapper = Delegate<Wrapper, PartialEq, inner> 
    ```

---

### How to Make This Work: The `Reflect` and `Rewrite` Macros

For these types to work without becoming a nightmare of edge cases, your language needs a compile-time API for structural manipulation. You should expose these types not as hidden compiler magic, but as standard library macros built on top of CTE and Reflection.

Here is how `Optionify` might be implemented in your language's macro system:

```rust
@cte // Runs at compile time
type_optionify<T, Fields> = {
    let original = reflect::<T>();
    let new_struct = Struct(original.name + "Partial");
    
    // 1. Transform Fields
    for field in original.fields {
        if fields.contains(field.name) {
            new_struct.add_field(field.name, Option<field.type>);
        } else {
            new_struct.add_field(field);
        }
    }

    // 2. Transform Methods using CTE AST rewriting
    for method in original.methods {
        let new_method = method;
        for field in Fields {
            // Rewrite `self.field` to `self.field.unwrap_or_default()`
            new_method.ast = rewrite(
                new_method.ast, 
                `self.{field}`, 
                `self.{field}.unwrap_or_default()`
            );
        }
        new_struct.add_method(new_method);
    }

    return new_struct;
}
```

### The Big Catch: Ownership and Lifetimes

Because your language is Rust-like, `Merge<A, B>` and `MapFields<T, _>` will hit lifetime issues immediately. 
* If `A` holds `&'a str` and `B` holds `i32`, what is the lifetime of `Merge<A, B>`? 
* The CTE macro system must be smart enough to collect all lifetime parameters from `A` and `B` and declare them on the generated struct.

**Summary of recommendations for your language:**
1. Include `Pick`, `Omit`, `Merge` for basic structural manipulation (with the method-dependency pruning you invented).
2. Include `MapFields` and `Optionify` (Partial), but implement them via **CTE AST rewriting** so methods don't break when field types change.
3. Include `Args`, `Return`, `Delegate` to bridge the gap between structural types and trait behavior.
4. Expose the `reflect` and `rewrite` APIs so users can build their own utility types!
