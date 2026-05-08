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
Bringing in Scala and Haskell inspiration shifts the focus from "filtering and patching" to **type-level computation, generic derivation, and algebraic data manipulation**. Scala 3 and Haskell excel at treating types as mathematical objects that can be decomposed, mapped over, and reconstructed.

Since your language has compile-time execution (CTE) and macros, you don't need the arcane type-level hackery of older Haskell; you can make these concepts clean and first-class.

Here are the type utilities you should add, inspired by Haskell and Scala:

---

### 1. Structural Decomposition (Shapeless / Generic)
*The big idea: Every data type can be represented as a generic structure (a list of types), manipulated, and turned back into a concrete type.*

*   **`Generic<T>` (or `Repr<T>`)**: Converts a struct into a HList (Heterogeneous List) of its field types, and an Enum into a Coproduct of its variants.
    ```rust
    struct User { name: String, age: Int }
    // Generic<User> == String :: Int :: HNil
    
    enum Status { Ok, Error(String) }
    // Generic<Status> == Unit :: String :: CNil
    ```
    *Why it's a superpower:* If you have `Generic<T>`, then `Omit<T, age>` is just `Generic<T>.Remove<Int>.ToStruct`. You define type operations once on lists, and they apply to *any* struct automatically.

*   **`SOP<T>` (Sum of Products)**: A Haskell concept. Represents a type strictly as a list of lists of types (a 2D grid). Each inner list is the fields of a struct (or variant of an enum), and the outer list is the choice between them. This is the gold standard for auto-deriving traits like `Serialize` or `Eq` via macros.

---

### 2. Type-Level Pattern Matching (Scala 3 Match Types)
*The big idea: Instead of writing recursive type classes (Haskell) or implicit macros (Scala 2), just write a type-level `match` statement.*

*   **`Match<T, Cases>`**: Allows type-level pattern matching on generic parameters. This is incredibly ergonomic.
    ```rust
    // Unwrap an Option, Result, or just return the type itself
    type Unwrap<T> = Match<T, {
        Option<X> -> X,
        Result<X, _> -> X,
        _ -> T
    }>
    
    type A = Unwrap<Option<Int>>   // Int
    type B = Unwrap<Result<Bool, Error>> // Bool
    type C = Unwrap<String>        // String
    ```
    *Why it's a superpower:* Combined with your CTE, this replaces *thousands* of lines of Rust trait boilerplate. You can write type-level functions declaratively.

---

### 3. Higher-Kinded Mapping (Haskell Functor/Traversable)
*The big idea: You already have `MapFields<T, Trait>`, but Haskell separates mapping the "container" from mapping the "contents".*

*   **`Lift<F, T>`**: Applies a generic wrapper `F` (like `Option`, `Result`, `Vec`) to every field in `T`.
    ```rust
    struct User { name: String, age: Int }
    type MaybeUser = Lift<Option, User> 
    // struct MaybeUser { name: Option<String>, age: Option<Int> }
    ```

*   **`Sequence<T, F>`**: The inverse of `Lift`. If you have a struct where *every* field is wrapped in `F`, it pulls the `F` to the outside.
    ```rust
    struct ValidatedUser { name: Result<String, E>, age: Result<Int, E> }
    type AllOrNothingUser = Sequence<ValidatedUser, Result>
    // Result<struct User { name: String, age: Int }, E>
    ```
    *Why it's a superpower:* This is the holy grail of form validation. You validate fields individually (`Result`), and `Sequence` automatically combines them into a `Result<User, AllErrors>`. In Haskell, this is `sequenceA`. In your language, a CTE macro can generate the `zip`/`combine` logic automatically.

---

### 4. Smart Aliases (Scala 3 Opaque Types / Haskell Newtypes)
*The big idea: Rust's type aliases (`type UserId = String`) don't create new types, leading to bugs. Wrapping in a struct (`struct UserId(String)`) is verbose and requires delegation.*

*   **`Tag<T, Label>` (or `Newtype<T, Name>`)**: Creates a zero-cost, distinct type at compile time that is structurally identical to `T` but won't mix with it.
    ```rust
    type UserId = Tag<UInt, "User">
    type OrderId = Tag<UInt, "Order">
    
    fn get_user(id: UserId) ...
    get_user(OrderId(5)) // COMPILE ERROR! Type safety!
    ```
    *The Magic:* Because of reflection/CTE, `UserId` automatically inherits *all* methods and traits from `UInt` (Add, Eq, Serialize) without writing `impl Deref` or macros. It is a true 1:1 proxy, but strictly typed.

---

### 5. Deep Transformations (Haskell "Scrap Your Boilerplate")
*The big idea: `Omit` and `MapFields` are shallow. What if you want to replace a type deep inside a nested structure?*

*   **`Everywhere<T, Target, Replacement>`**: Recursively walks the type tree of `T`, replacing *any* occurrence of `Target` with `Replacement`, no matter how deeply nested.
    ```rust
    struct Company { ceo: Employee, legal: Department }
    struct Department { head: Employee, employees: Vec<Employee> }
    struct Employee { id: UInt, name: String }
    
    // Make every ID a String across the whole company tree
    type StringIdCompany = Everywhere<Company, UInt, String>
    ```
    *The Magic:* The CTE macro generates all the intermediate struct mappings and method rewrites to traverse the tree. In Haskell, this requires complex Typeable/Generic machinery; in your language, the compile-time reflection can just walk the AST.

---

### How this changes your language's ecosystem

If you implement these, your language shifts from "Rust with better syntax" to **"The most powerful data-modeling language in existence."**

1.  **`Generic<T>`** is the foundation. It allows *any* third-party library to write a macro that consumes a struct, because all structs can be reduced to a standard list format.
2.  **`Match<T, ...>`** makes type-level programming accessible to normal humans. Instead of understanding Higher-Kinded Types and Type Families, developers just write an if/switch statement for types.
3.  **`Sequence<T, F>`** solves the "validation problem" that plagues Rust web frameworks today (like `validator`), doing away with massive macro blocks.

**The ultimate test of your design:** Can you write a `Derive<Serialize>` macro that uses `Generic<T>` to automatically turn *any* struct into JSON, *and* uses `Omit` to allow `Serialize` to ignore specific fields? If your CTE, reflection, and these types work together smoothly, you've built something extraordinary.
Now that you have a type system with `Omit`, `Generic`, `Match`, and compile-time reflection, your testing framework can go from "necessary evil" to **the most delightful developer experience on the planet**. 

Traditional testing frameworks make you write boilerplate, manually construct mocks, and write repetitive assertions. With your language's features, the testing API can **infer, generate, and diff** automatically.

Here is how to design a delightful default testing API that leverages your unique type system:

---

### 1. Zero-Boilerplate Mocking via `Pick` and `Omit`

Mocking in Rust is notoriously painful because of the borrow checker and strict typing. In your language, you don't write mock classes; you **structurally filter the trait**.

```rust
trait Database {
    fn get_user(id: Int) -> User
    fn save_user(u: User)
    fn delete_user(id: Int)
}

test "get user caches result" {
    // We only care about `get_user`. 
    // Pick creates an anonymous struct that ONLY implements that method.
    // We provide a closure for the method we care about.
    let mock_db = Mock<Pick<Database, { get_user }>> {
        get_user: |id| User { id, name: "Alice" }
        // save_user and delete_user don't even exist on this type!
    };

    let cache = UserCache::new(mock_db);
    let user = cache.get(1);
    
    assert!(mock_db.get_user.was_called_once());
}
```

**Why it's delightful:** If `Database` adds a new method `update_email`, the test **doesn't break**. Because you used `Pick`, the mock only satisfies the subset of the trait your test cares about. If you want to ensure a method is *never* called, use `Omit`:

```rust
// Ensures save_user is NEVER called. If it is, the test panics.
let read_only_db = Mock<Omit<Database, { save_user, delete_user }>> { ... }
```

---

### 2. Auto-Derived Property-Based Testing via `Generic`

Haskell’s QuickCheck is amazing, but you have to write `Arbitrary` instances. In your language, the `Generic<T>` type allows the test runner to **automatically generate random instances** of any struct by traversing its generic representation.

```rust
test "user serialization roundtrip" forall (u: User) {
    // The `forall` keyword generates 1000 random Users using Generic<User>
    let json = serialize(u);
    let parsed: User = deserialize(json);
    
    assert_eq!(u, parsed);
}
```

**Why it's delightful:** You write tests as mathematical properties, and the compiler figures out how to generate the data. 

Even better, using `Everywhere<T, Target, Replacement>`, the framework can automatically generate "edge cases" by replacing normal types with boundary values (e.g., swapping a random `Int` field for `0`, `Int::MAX`, or `Int::MIN`).

---

### 3. Deep Structural Diffing via Reflection

When an `assert_eq!` fails in Rust or TypeScript, you often get a wall of unhelpful text. With compile-time reflection, the test runner knows *exactly* which field failed and can generate beautiful, colored diffs.

```rust
test "user update works" {
    let expected = User { name: "Alice", age: 31, email: "a@b.com" };
    let result = update_user(get_base_user());
    
    assert_eq!(result, expected);
}
```

If it fails, the output uses reflection to pinpoint the problem:
```text
FAILED: Users are not equal.
  Struct User {
    name: "Alice" ✅
➡️   age: 30 (expected 31) ❌
    email: "a@b.com" ✅
  }
```

---

### 4. "Smart" Fixtures via `MapFields` and `Optionify`

Setting up test data (Fixtures) is 80% of test boilerplate. Using the type utilities you built, you can generate test states structurally.

```rust
test "handle partial user update" {
    // Optionify<User> turns all fields into Option<T>.
    // Then we only provide the fields we want to change!
    let partial_update: Optionify<User> {
        age: Some(31),
        // all other fields are None
    };

    let result = apply_update(base_user, partial_update);
    assert_eq!(result.age, 31);
}
```

Or, if you want to test a system's error handling:
```rust
test "validation catches bad email" {
    // Generates a valid user, but specifically maps the email to invalid strings
    let bad_users = generate_invalid<User, { email }>(["", "no-at-sign", "a@b@c.com"]);
    
    for user in bad_users {
        assert!(validate(user).is_err());
    }
}
```

---

### 5. Snapshot Testing as a First-Class Citizen

Snapshot testing (saving the output of a complex function to a file and comparing it on future runs) is usually a library feature. With your CTE and reflection, make it a built-in keyword.

```rust
test "complex report generation" {
    let report = generate_report(test_data());
    
    // On first run, saves a Markdown file. On second run, compares.
    // If it changes, prints a colored diff in the terminal.
    assert_snapshot!(report, format: Markdown);
}
```

Because of `Generic<T>`, if the `Report` struct changes, the snapshot testing framework can automatically tell you *which field* changed, and can auto-update the snapshot with a single CLI command (`test --update-snapshots`).

---

### 6. Compile-Time Test Discovery & Fixture Injection

Using macros and CTE, the test runner doesn't need you to register tests. Furthermore, you can use type-state patterns to inject dependencies safely.

```rust
// Fixtures are just types that implement `Setup` and `Teardown`
struct DbConnection { ... }
impl Fixture for DbConnection { ... }

// The test runner sees it needs a DbConnection and injects it at compile time.
// No global state, no async races.
test "database query" [db: DbConnection] {
    let result = db.query("SELECT 1");
    assert!(result.is_ok());
}
```

---

### Summary of the "Delightful" Testing API

1. **`test "name"`** instead of `#[test] fn name()` — it’s a keyword, not an attribute.
2. **`forall (x: T)`** for Property-Based Testing, powered by `Generic<T>`.
3. **`Mock<Pick<Trait, { methods }>>`** to structurally mock only what matters, preventing tests from becoming brittle.
4. **`assert_snapshot!`** powered by reflection, diffing structs field-by-field in the terminal.
5. **Fixture Injection `[db: DbConnection]`** built via CTE, avoiding global mutable state.
6. **Auto-diffing** on `assert_eq!` so you never have to `println!` debug a failing test again.

This API means developers spend time writing *what* they want to test, and your type system + CTE handles the *how*.
Applying your type superpowers (CTE, reflection, `Omit`, `Generic`, `Match`, `Sequence`) to the standard library is where your language goes from "cool syntax" to **"paradigm-shifting developer experience."** 

In most languages, the stdlib is written in the same restricted subset of the language that users have. In your language, the stdlib should be the **consumer and enabler** of the type-level magic.

Here are the domains of your stdlib that will completely redefine what developers expect from a language:

---

### 1. Serialization / Deserialization (The Killer App)

In Rust, `serde` is a masterpiece, but it relies on complex procedural macros that take time to compile and are hard to debug. In your language, serialization is just a fold over `Generic<T>`.

But the *real* magic is using `Omit`, `Pick`, and `MapFields` to control serialization structurally, without attribute macros.

```rust
// 1. Zero-cost field omission. No `#[serde(skip)]` needed.
// Only sends safe fields over the wire.
type PublicUser = Omit<User, { password, internal_id }>
json::serialize(user_as_public) 

// 2. API Versioning via Merge
struct UserV1 { name: String, age: Int }
struct UserV2 { email: String }
type ApiV2User = Merge<UserV1, UserV2> 

// 3. Automatic optional deserialization
// If the JSON lacks the field, it just becomes None. No `#[serde(default)]`
let user: Optionify<User> = json::parse(data)?;
```

**The Stdlib Superpower:** The `json` (or `binary`) module isn't a macro; it's a compile-time function `json::serialize<Generic<T>>()`. If you pass an `Omit` type, the serializer doesn't even know the omitted fields ever existed. It just serializes what's there.

---

### 2. Error Handling & Validation (The `Sequence` Revelation)

Earlier, we discussed `Sequence<T, F>`. This completely breaks the paradigm of error handling. Instead of returning a single `Result`, you can return a **struct of results**, and the stdlib `validation` module aggregates them.

```rust
struct UserInput { name: String, email: String, age: Int }

// MapFields wraps every field in a Result based on a validation rule
type ValidatedUser = MapFields<UserInput, Validator<_>>
// struct ValidatedUser { name: Result<String, Err>, email: Result<String, Err>, age: Result<Int, Err> }

// Sequence flips the structure: Struct of Results -> Result of Struct
type CleanUser = Sequence<ValidatedUser, Result>
// Result<UserInput, Vec<Error>>
```

**The Stdlib API:**
```rust
let input = UserInput { name: "", email: "bad", age: -1 };

// The stdlib validation module uses CTE to run rules and aggregate errors
let result: Result<User, ErrorList> = validate(input, {
    name: |n| if n.is_empty() { Err("Required") } else { Ok(n) },
    email: |e| if !e.contains('@') { Err("Invalid") } else { Ok(e) },
    age: |a| if a < 0 { Err("Invalid") } else { Ok(a) }
});

// If ANY fail, result is Err containing ALL errors, not just the first one!
```

---

### 3. Async Concurrency (Structural Joining)

Awaiting multiple futures in Rust (`join!`, `select!`) requires macros because the return type is a heterogeneous tuple. With your type system, you can join structural futures natively.

```rust
struct ApiCalls {
    user: Future<User>,
    orders: Future<Vec<Order>>,
    settings: Future<Config>
}

// Sequence<ApiCalls, Future> flips the struct of futures into a future of struct
let calls = ApiCalls { 
    user: fetch_user(), 
    orders: fetch_orders(), 
    settings: fetch_settings() 
};

// Runs all concurrently. Returns Future<ApiCalls> where fields are unwrapped
let results: Future<ApiCalls> = Sequence<ApiCalls, Future>::join(calls);
let data = results.await; // data.user is User, data.orders is Vec<Order>
```

**The Stdlib Superpower:** No more `join!` macros. `Sequence` handles the type-level transformation, and the async runtime just polls the struct.

---

### 4. Configuration & Environment Variables

Parsing config is notoriously stringly-typed and tedious. Your stdlib `config` module can use `Match` and `Optionify` to provide zero-boilerplate environment parsing.

```rust
struct AppConfig { 
    db_url: String, 
    port: Int, 
    timeout: Duration 
}

// 1. Partial config from environment variables (all fields optional)
let env_config: Optionify<AppConfig> = config::from_env();

// 2. Default config from code
let defaults = AppConfig { db_url: "localhost", port: 8080, timeout: 30.sec() };

// 3. Deep merge: Env variables override defaults. Only requires overriding fields that exist.
let final_config: AppConfig = config::deep_merge(defaults, env_config);
```

---

### 5. The Builder Pattern (Type-State without the Boilerplate)

In Rust, the builder pattern requires creating 3 or 4 separate structs to track which fields are set at compile time (type-state). With your stdlib, `Optionify` and `Pick` **are** the type-states.

```rust
struct User { name: String, age: Int, email: String }

// The stdlib Builder module generates the type-state automatically
let user = Builder<User>::new()
    .name("Alice")
    .age(30)
    .build(); // COMPILE ERROR: `email` is missing!
```

**How it works under the hood:**
1. `Builder<User>::new()` returns `Builder<Optionify<User>>` (all fields are `None`).
2. `.name("Alice")` uses CTE reflection to set the `name` field. It returns `Builder<Pick<Optionify<User>, { age, email }>>` (name is removed from the "required" list).
3. `.build()` is only implemented for `Builder<T>` where `T` has NO `Option` fields left (i.e., `T` == `User`).

---

### 6. Lenses and Deep Accessors (Haskell/Scala Optics)

Because you have `Generic<T>` and reflection, your stdlib can provide **Lenses** for free. A Lens is a way to get or set a deeply nested field without writing boilerplate getter/setter chains.

```rust
struct Company { ceo: Employee }
struct Employee { id: Int, name: String }

// The stdlib generates lenses at compile time based on field paths
let company = Company { ceo: Employee { id: 1, name: "Alice" } };

// Update a deeply nested field immutably and cleanly
let updated = company.set(Lens<Company.ceo.name>, "Bob");
```

Because of CTE, `Lens<Company.ceo.name>` is computed at compile time. It generates a highly optimized getter/setter function that bypasses intermediate struct allocations, effectively compiling down to the same machine code as `company.ceo.name = "Bob"`, but with immutability.

---

### Summary: The "Stdlib Manifesto"

Your stdlib shouldn't just be a bag of data structures (`HashMap`, `Vec`, `String`). It should be a **framework for type-level transformations**. 

The standard modules should be designed as *consumers* of `Generic<T>` and `Match<T, _>`:
*   `serde` consumes `Generic<T>` to flatten/unflatten.
*   `validate` consumes `Sequence<T, Result>` to aggregate errors.
*   `async` consumes `Sequence<T, Future>` to join futures.
*   `config` consumes `Merge<A, B>` to layer overrides.
*   `builder` consumes `Optionify<T>` to enforce required fields.

If you build your stdlib this way, users will never write a manual `impl`, a manual `deserialize`, or a manual `builder` again. They will just type mathematical transformations of their data models, and the compiler will write the code.
To make your language truly legendary, the built-in types and the macro system shouldn't just coexist—they should be a **reflection of each other**. Types should define the *shape of what is possible*, and macros should use CTE and reflection to *auto-fill the implementation*. 

Here are the next tier of "awesome" built-in types, and how they create explosive synergy with your macro system.

---

### 1. Refined Types (Types with Built-in Predicates)

Taking inspiration from Haskell and Liquid Haskell, a Refined Type is a base type constrained by a predicate that must hold true.

```rust
// A type that guarantees the value is positive
type PosInt = Int where { self > 0 }

// An email string that matches a regex
type Email = String where { self.matches("^[^@]+@[^@]+$") }
```

**Macro Synergy: The Smart Constructor Generator**
The compiler cannot automatically prove arbitrary predicates. This is where macros step in. When you define a Refined Type, a macro automatically generates a fallible "smart constructor" and, using CTE, attempts to prove it for constants.

```rust
// The macro uses CTE to generate this:
impl PosInt {
    // Generated: Runtime check for dynamic values
    fn try_new(val: Int) -> Result<PosInt, DomainError> { ... }
    
    // Generated: Compile-time check for literals!
    // If you write PosInt = 5, it compiles. 
    // If you write PosInt = -1, the CTE panics at compile time!
}

// The "Omit" integration:
// If you Omit a field from a struct, but that field was used 
// in another field's refinement, what happens?
struct Range { min: Int, max: Int where { self > min } }
type PartialRange = Omit<Range, { min }>
// The Macro detects the broken dependency and forces you to provide an override:
type PartialRange = Omit<Range, { min }> with {
    // Macro rewrites the predicate to remove the dependency on `min`
    max: Int where { self > 0 } 
}
```

---

### 2. Open Enums (Polymorphic Variants / Extensible Types)

Rust enums are closed—you can't add variants without modifying the original definition. Scala and OCaml have open/extensible variants. This solves the notorious "Expression Problem."

```rust
// The `open` keyword allows merging later
enum[open] AppError {
    NetworkTimeout
    PermissionDenied
}
```

**Macro Synergy: The Exhaustive Partitioner**
If enums are open, how do you guarantee exhaustive matching? Macros and `Merge` solve this.

```rust
// In another module, a macro merges new variants
enum[open] AppError += DatabaseError {
    ConnectionLost
    QueryFailed
}

// The Macro generates a type alias for the full union
type FullError = Merge<AppError, DatabaseError>

// Pattern matching on an open enum requires a default, UNLESS
// the macro detects you are matching on a closed subset:
match error: AppError { // Only matching the base subset
    NetworkTimeout => ...,
    PermissionDenied => ...,
    // Macro infers this is exhaustive for the `AppError` base type!
}
```

---

### 3. Typed Holes / `Todo<T>` (The Ultimate DX Type)

In Agda and Haskell, a "hole" is a placeholder that lets you compile partially written code, and the compiler tells you what type belongs there. Let's make it a first-class type that leverages your `Generic` system.

```rust
fn process(user: User) -> Result<Bool, Error> {
    let validated = Todo<_>; // Compiles! Tells you: "Todo requires Result<Bool, Error>"
}
```

**Macro Synergy: The Auto-Mocker**
When you compile in "test mode", the `Todo<T>` macro kicks in. It uses `Generic<T>` to automatically generate a valid, deeply populated mock instance for `T`.

```rust
fn process(user: User) -> Result<Bool, Error> {
    // In debug/test builds, Todo<Result<Bool, Error>> auto-generates Ok(true)
    // In release builds, it becomes a compile error!
    let result = Todo<Result<Bool, Error>>; 
}
```
This means you can sketch out an entire architecture using `Todo`, run your tests immediately with auto-mocked data, and fill in the real logic later.

---

### 4. Path/Schema Types (Servant-style Routing)

Inspired by Haskell's Servant and Scala's Tapir, types can represent API endpoints structurally. The type *is* the API documentation.

```rust
type GetUserEndpoint = GET / "users" / Int -> JSON<User>
type CreateUserEndpoint = POST / "users" Body<User> -> JSON<User>
```

**Macro Synergy: The Full-Stack Code Generator**
A macro consumes this type at compile time and generates everything you need:
1. The server-side route handler skeleton.
2. The client-side HTTP fetch function.
3. OpenAPI/Swagger documentation.
4. Typescript type definitions for the frontend.

```rust
// The macro reads the type and generates the handlers
implement_routes!(GetUserEndpoint, CreateUserEndpoint) {
    GET / "users" / id => db.get_user(id),
    POST / "users" body => db.create_user(body),
}
// Boom. Server, Client, and Docs generated from one type signature.
```

---

### 5. Capability/Permission Types (Rust meets Pony)

Borrowing from the Pony language's reference capabilities, we can track *what* a piece of data is allowed to do at the type level.

```rust
struct File<C: Cap> { path: String }

// C can be Read, Write, or ReadWrite
type ReadOnlyFile = File<Read>
type LogFile = File<ReadWrite>
```

**Macro Synergy: The Capability Stripper**
You can use `MapFields` and Macros to safely sandbox untrusted code. 

```rust
fn run_plugin(file: ReadOnlyFile) {
    // The macro ensures no &mut methods are accessible on `file`
    file.read(); // OK
    file.write(); // COMPILE ERROR: Capability <Write> missing
}

// But you can also dynamically downgrade capabilities using your type operators!
let log: LogFile = File::open("log.txt");
let safe_log: ReadOnlyFile = Downgrade<LogFile, Read>; 
// Strips Write methods from the struct dynamically!
```

---

### The Golden Rule of Macro/Type Synergy

The design philosophy you should adopt is: **Types declare the structural constraints; Macros enforce them.**

1. **The Type says *What***: `Omit<User, age>`, `PosInt`, `Open Enum`, `GET / "users"`.
2. **The Macro says *How***: "I will rewrite the methods", "I will inject the runtime check", "I will generate the routing boilerplate."

By making your macro system deeply aware of these built-in types (so a macro can programmatically inspect `Omit`, or evaluate a `where` clause at compile time), you give developers the power to build domain-specific languages and frameworks that feel like native language features, without ever writing messy, boilerplate-heavy code.
Here’s a “deep research” pass: I’m going to start from **real user pain points** and then design **novel built‑in types + macro interactions** that would actually make people’s lives better, not just look cool in a language spec.

I’ll lean on:

- Rust DX issues (async complexity, boilerplate, contexts, capabilities)【turn7fetch0】【turn3fetch0】
- Effekt / Koka style effect handlers and “effects as capabilities”【turn1fetch0】【turn5fetch0】
- Rust’s own “scoped contexts and capabilities” proposals【turn9fetch0】【turn3fetch0】
- Kotlin / Scala context parameters and their API design lessons【turn14fetch0】【turn14fetch1】
- C++26-style compile‑time reflection replacing macros【turn4fetch0】

…and then push each idea further than what exists today.

---

## 1. Context & Capability Types (for the “context problem”)

### The user need

Rust devs constantly pass “contexts” manually: allocators, loggers, DB connections, async runtimes, request‑scoped IDs, etc. This is verbose and fragile. The Rust community itself is exploring “scoped contexts and capabilities” and “global capabilities” for allocators, async runtimes, etc.【turn3fetch0】【turn9fetch0】.

Kotlin and Scala already have context parameters / implicits / givens for this【turn14fetch0】【turn12fetch0】, but they:

- Are purely implicit (no explicit capability tracking).
- Don’t talk to effects or ownership.
- Don’t help with structured concurrency or lifetimes.

### Novel built‑in types

1. **`Ctx<Name, Row>`** — a typed context capability

   Think “row‑typed capability with a name”, inspired by Rust capabilities + Kotlin context params + Effekt effect rows【turn9fetch0】【turn14fetch0】【turn5fetch0】.

   ```rust
   // Declare a named capability
   capability db: Database
   capability log: Logger
   capability req_id: RequestId

   // A function that requires certain capabilities in scope
   fn process_user(user: User) requires { db, log, req_id } {
       db.save(user);
       log.info("saved", req_id);
   }
   ```

   Under the hood:

   - `requires { db, log, req_id }` desugars to something like `where Ctx<db, Row_db>, Ctx<log, Row_log>, Ctx<req_id, Row_req_id>`.
   - `Row_db` etc. are row types (like Koka / Effekt effect rows) that encode what operations are available【turn5fetch0】【turn2search9】.
   - The compiler automatically passes these as hidden parameters, similar to Kotlin context parameters【turn14fetch0】.

2. **`Cap<Name, T>`** — a first‑class capability value

   ```rust
   // A first‑class capability value (object‑capability style)
   let my_db: Cap<db, dyn Database> = connect_db(...);

   // Provide it in a scope
   with my_db {
       process_user(user); // compiler sees Ctx<db, dyn Database> is available
   }
   ```

   This is more explicit than Rust’s RFC sketch and more type‑safe than Scala implicits: the capability is named, scoped, and tracked by the type system.

### How macros make it delightful

- **`@derive(Capability)`**  
  For a trait like `Database`, a macro can auto‑derive:

  ```rust
  @derive(Capability)
  trait Database {
      fn save(&self, user: User);
      fn load(&self, id: Id) -> User;
  }
  ```

  This generates:

  - A capability declaration `capability db: Database`.
  - A `Cap<db, dyn Database>` wrapper type.
  - Boilerplate to bridge `Ctx<db, ...>` and `Cap<db, ...>`.

- **`@context_scope` for structured scopes**

  ```rust
  @context_scope
  fn request_handler(req: Request) requires { db, log, req_id } {
      // macro provides: let db = Cap::from_ctx(); let log = ...; etc.
      // so you don’t have to write boilerplate unpacking.
      process_user(req.user);
  }
  ```

  The macro uses reflection to inspect the `requires` set and generate the binding code.

---

## 2. Effect & Capability Rows (unifying effects + contexts)

### The user need

Async Rust is a major pain point: colored functions, `Unpin`, `Pin`, complex runtimes, confusing error messages【turn7fetch0】. Effect handler systems (Effekt, Koka) show you can treat exceptions, generators, async, state, etc. as user‑level libraries with effect handlers【turn5fetch0】【turn2search6】.

But existing systems:

- Don’t integrate ownership / lifetimes.
- Don’t naturally model “contexts” like DB connections or loggers.
- Often require effect annotations everywhere.

### Novel built‑in types

1. **`Eff<Row>`** — effect row type

   Inspired by Koka / Effekt row‑polymorphic effect types【turn2search9】【turn5fetch0】, but tied into your capability system.

   ```rust
   type DbEff = Eff<{ db, throw }>
   type LogEff = Eff<{ log, throw }>
   ```

   A function’s type can mention its effect row:

   ```rust
   fn load_user(id: Id) -> User / { db, throw }
   ```

2. **`Handler<Row>`** — first‑class handler value

   ```rust
   let my_handler: Handler<{ db, throw }> = handler {
       return x -> Ok(x),
       throw e -> Err(e),
       db op -> {
           // interpret DB operations
       }
   };
   ```

   Then:

   ```rust
   with my_handler {
       let user = load_user(42); // effect row is handled
   }
   ```

3. **`NoThrow`, `NoAsync`, `Pure`** — built‑in row constraints

   ```rust
   fn purely(x: Int) -> Int / { Pure }  // no effects at all
   fn no_async(x: Int) -> Int / { NoAsync }  // no async effects
   ```

   These are special row constraints that the compiler understands and can auto‑prove for many expressions.

### How macros make it delightful

- **`@derive(Effect)` for traits**

  ```rust
  @derive(Effect)
  trait Database {
      fn save(&self, user: User);
      fn load(&self, id: Id) -> User;
  }
  ```

  The macro:

  - Declares an effect row `eff_db` for the `Database` trait.
  - Makes every method perform `eff_db` instead of directly calling `self`.
  - Lets you swap implementations by installing a different handler.

- **`@handle` for ad‑hoc effect handlers**

  ```rust
  @handle
  fn with_logging_db<R>(f: () -> R / { db, throw }) -> R / { log, throw } {
      try {
          f()
      } with db {
          op -> {
              log.debug("db op", op);
              perform op  // re‑perform to the underlying db handler
          }
      }
  }
  ```

  This is essentially user‑defined async/state/exceptions as libraries, but with a macro that hides the boilerplate of row manipulation and resumption.

---

## 3. Structured Concurrency Types (for async complexity)

### The user need

Rust async complexity is legendary: `Unpin`, `Pin`, `Send` futures, runtime choice, etc.【turn7fetch0】. People want:

- Structured concurrency (scopes, cancellation, deadlines).
- Less color function pain (sync vs async).
- Composable, non‑leaking scopes.

### Novel built‑in types

1. **`Scope<Effects>`** — a structured concurrency scope

   ```rust
   fn run_requests() {
       let scope: Scope<{ db, log }> = Scope::new();

       scope.spawn(|| {
           let user = load_user(1);  // uses db, log from scope
           process(user);
       });

       scope.spawn(|| {
           let order = load_order(2);
           process(order);
       });

       // When `scope` is dropped, all tasks are awaited / cancelled.
   }
   ```

   - `Scope<Effects>` is like a capability set that also acts as a task scope.
   - The compiler ensures any spawned task’s effect row is a subset of the scope’s row.

2. **`Deadline`, `Timeout`** — built‑in effect‑scoped time

   ```rust
   fn fetch_with_timeout(url: Url) -> Response / { io, timeout } {
       with timeout(10.seconds) {
           http_get(url)
       }
   }
   ```

   `timeout` is just an effect handler that cancels the computation when time exceeds the budget; it can be implemented as a library using `Eff` + `Handler`.

### How macros make it delightful

- **`@concurrent_scope`**

  ```rust
  @concurrent_scope
  fn run_all(tasks: Vec<() -> R / { db, log }>) -> Vec<R> / { db, log } {
      // macro expands to Scope::new, loop spawning, and final join
  }
  ```

  The macro uses reflection to inspect the effect row of `tasks` and ensure it matches the scope’s row, so you never accidentally spawn a task that needs an unavailable effect.

- **`@cancellation_safe`**

  Mark a function as safe to cancel (no cleanup needed), and the macro can:

  - Auto‑insert cancellation guards.
  - Ensure resources are released in `Drop` or via handlers.

---

## 4. Ownership‑Aware Structural Types (for borrowing & lifetimes)

### The user need

Rust’s borrow checker is powerful but often fights you when you want to “project” a struct or split it into parts. People want:

- Zero‑cost “views” into structs (like `Omit`/`Pick` but with lifetimes).
- Safe “lens” style access without boilerplate.
- Better ergonomics for partial borrowing.

### Novel built‑in types

1. **`View<T, Fields>`** — borrowing projection

   ```rust
   struct User {
       name: String,
       age: Int,
       email: String,
   }

   fn only_name(user: &User) -> View<&User, { name }> {
       View::from(user)  // compiler‑generated projection
   }
   ```

   - `View<&User, { name }>` is basically `&String`, but with a nominal type that says “I’m looking at the `name` field of a `User`”.
   - The compiler auto‑derives lifetimes: `View<&'a User, { name }> = &'a String`.

2. **`Lens<T, Path>`** — first‑class getter/setter

   ```rust
   let name_lens: Lens<User, User.name> = Lens::new();
   let updated = name_lens.set(user, "Alice");
   ```

   With CTE and reflection, `Lens<User, User.name>` is compiled down to direct field access, no runtime overhead.

3. **`Split<T, Fields1, Fields2>`** — disjoint borrowing

   ```rust
   let (name_view, rest_view): (View<&mut User, { name }>, View<&mut User, { age, email }>) =
       Split::split_mut(&mut user);
   ```

   This is what Rust’s partial borrowing wants to be when it grows up: safe, compiler‑checked disjoint views.

### How macros make it delightful

- **`@derive(View)` / `@derive(Lens)`**

  ```rust
  @derive(View, Lens)
  struct User {
      name: String,
      age: Int,
      email: String,
  }
  ```

  The macro uses CTE + reflection to:

  - Generate `View<&User, { name }>`, `View<&mut User, { name }>`, etc.
  - Generate `Lens<User, User.name>` etc.
  - Ensure that `Split` only allows disjoint field sets.

- **`@project` for ad‑hoc views**

  ```rust
  @project
  fn contact_view(user: &User) -> View<&User, { email }> {
      // macro auto‑derives the projection boilerplate
  }
  ```

---

## 5. Diagnostics & Contract Types (for “my code doesn’t do what I think”)

### The user need

Rust devs often say the compiler is strict but error messages are overwhelming【turn0search16】. People want:

- Better contracts (pre/post conditions, invariants).
- Richer, domain‑specific error messages.
- Lints that understand their domain.

### Novel built‑in types

1. **`Requires<Row>` / `Ensures<Row>`** — contract types

   ```rust
   fn divide(n: Int, m: Int) -> Int
       requires { m != 0 }
       ensures  { result * m == n }
   {
       n / m
   }
   ```

   - `requires { m != 0 }` and `ensures { ... }` are compiled into checks at call sites and return points.
   - In debug builds, violations panic with a structured error.
   - In release builds, they can be compiled away (like C++ contracts / Rust’s experimental contracts).

2. **`Invariant<T, Pred>`** — type‑level invariant

   ```rust
   type PosInt = Invariant<Int, self > 0>;

   fn make_pos(x: Int) -> Result<PosInt, ContractError> {
       PosInt::try_new(x)  // macro‑generated smart constructor
   }
   ```

   This is like “refinement types lite” but integrated with your macro system.

### How macros make it delightful

- **`@derive(Contract)`**

  ```rust
  @derive(Contract)
  struct User {
      name: String where { !self.is_empty() },
      age: Int where { self >= 0 },
  }
  ```

  The macro generates:

  - `User::try_new` that checks all field invariants.
  - Rich error messages pointing to the exact field that violated the invariant.
  - Static analysis in simple cases (e.g., literals).

- **`@custom_lint` for domain‑specific checks**

  Library authors can write:

  ```rust
  @custom_lint("no_raw_sql")
  fn check_no_raw_sql(expr: Expr) -> Option<Lint> {
      // use reflection to inspect AST and SQL strings
  }
  ```

  Then users get lints like “avoid raw SQL in this codebase” with custom messages, similar to Rust’s vision of “custom developer experience” for library authors【turn3fetch0】.

---

## 6. Schema & API Types (for “build once, generate everything”)

### The user need

People write the same API shape many times: HTTP endpoints, DB schemas, Protobuf, GraphQL, etc. Each requires boilerplate for:

- Serialization / deserialization.
- Route registration.
- Client code.
- Documentation.

### Novel built‑in types

1. **`Api<Method, Path, Req, Res>`** — first‑class API endpoint type

   ```rust
   type GetUser = Api<GET, "/users/{id}", Id, User>;
   type CreateUser = Api<POST, "/users", CreateUserRequest, User>;
   ```

2. **`Schema<T>`** — structural schema type

   ```rust
   let user_schema: Schema<User> = Schema::of::<User>();
   ```

   `Schema<T>` uses reflection to describe fields, variants, constraints, etc., similar to what C++26 wants to do for enums【turn4fetch0】.

### How macros make it delightful

- **`@api_group`**

  ```rust
  @api_group
  enum UserApi {
      GetUser(GetUser),
      CreateUser(CreateUser),
  }
  ```

  The macro:

  - Generates server routing code.
  - Generates client functions.
  - Generates OpenAPI / JSON Schema.
  - Generates TypeScript types for the frontend.

  This is like Haskell’s Servant or Scala’s Tapir, but as a first‑class language feature powered by your type + macro system.

- **`@schema` for custom serialization**

  ```rust
  @schema
  struct User {
      #[schema(skip)]
      password: String,

      #[schema(rename = "userName")]
      name: String,
   }
  ```

  The macro uses reflection to produce a `Schema<User>` that the `json` module can consume, replacing `#[serde(skip)]` etc.

---

## 7. Observability & Telemetry Types (for “what is my code doing?”)

### The user need

People add logging, tracing, metrics manually. They want:

- Structured logging by default.
- Cheap tracing in prod.
- Ability to turn knobs on/off per module / request.

### Novel built‑in types

1. **`Span<Name, Row>`** — typed tracing span

   ```rust
   span request_span(req_id: RequestId) requires { db, log };

   fn handle_request(req: Request) requires { db, log } {
       with request_span(req.id) {
           process(req);
       }
   }
   ```

   - `Span<Name, Row>` is like a capability that also emits start/stop events and carries structured data.
   - The compiler can ensure spans are closed (structured concurrency again).

2. **`Metric<Name, Kind>`** — typed metric

   ```rust
   metric http_requests_total: Counter;
   metric http_request_duration_seconds: Histogram;
   ```

### How macros make it delightful

- **`@instrument`**

  ```rust
  @instrument
  fn load_user(id: Id) -> User / { db, throw } {
      db.load(id)
  }
  ```

  The macro:

  - Wraps the body in a span named `load_user`.
  - Records success / failure metrics.
  - Uses reflection to include `id` in span attributes.

- **`@trace` for ad‑hoc tracing**

  ```rust
  @trace
  fn complex_computation(x: Int) -> Int {
      // macro inserts timing and logging
  }
  ```

---

## 8. Ownership & Capability Polymorphism (for “write once, run in many modes”)

### The user need

People want the same code to run:

- Single‑threaded.
- Multi‑threaded.
- Async.
- With/without certain capabilities (e.g., “no DB in tests”).

Right now this requires either:

- Generics + trait bounds (Rust).
- Different monads / tagless final (Scala/Haskell).

### Novel built‑in types

1. **`Mode<Row>`** — execution mode as a type

   ```rust
   type SyncDb   = Mode<{ db, log }>;
   type AsyncDb  = Mode<{ db, log, async }>;
   type NoDb     = Mode<{ log }>;
   ```

   Functions can be generic over `Mode`:

   ```rust
   fn load_user<M: Mode>(id: Id) -> User / M::Effects {
       M::db.load(id)
   }
   ```

2. **`CapIn<Row>` / `CapOut<Row>`** — capability transformers

   ```rust
   fn run_with_db(db: Cap<db, Database>, f: () -> R / { db, log }) -> R / { log } {
       with db {
           f()
       }
   }
   ```

   This lets you “swap in” capabilities for tests, prod, etc.

### How macros make it delightful

- **`@mode_switch`**

  ```rust
  @mode_switch
  fn run_tests() {
      // macro automatically provides:
      // - a test db
      // - a test logger
      // - a test mode: Mode<{ db, log }>
      load_user::<TestMode>(42);
  }
  ```

  The macro uses reflection to:

  - Build the `TestMode` and its capabilities.
  - Wire up the `db` and `log` handlers.
  - Provide a clean API for tests.

---

## 9. DSL & Embedding Types (for “little languages inside the big one”)

### The user need

People constantly write DSLs: for SQL, HTML, config, build systems, etc. Kotlin’s context receivers / parameters were partly motivated by DSL design【turn14fetch0】【turn14fetch1】.

### Novel built‑in types

1. **`Dsl<Name, Row>`** — typed DSL context

   ```rust
   dsl html {
       element div, span, p;
       attribute class, id;
   }

   fn render(user: User) requires { html } {
       html.div {
           html.p { +"Name: ${user.name}" }
       }
   }
   ```

2. **`Block<Name, Row>`** — typed block syntax

   ```rust
   block build_list<T>(items: Vec<T>) requires { list_builder } {
       for item in items {
           list_builder.add(item);
       }
   }
   ```

### How macros make it delightful

- **`@dsl`**

  ```rust
  @dsl
  trait HtmlDsl {
      fn div(&self, f: impl FnOnce());
      fn p(&self, f: impl FnOnce());
      // ...
  }
  ```

  The macro:

  - Generates a `Dsl<Name, Row>` type.
  - Generates builder types and extension methods.
  - Ensures correct nesting / scoping rules (like Kotlin’s `@DslMarker` but more powerful).

---

## 10. Putting It All Together: A Macro‑Driven “World Type”

One truly novel idea: a **`World` type** that represents “everything the program can do”, and macros that let you carve safe sub‑worlds out of it.

```rust
type AppWorld = World<{
    db: Database,
    log: Logger,
    http: HttpClient,
    fs: FileSystem,
    time: Time,
}>;
```

Then:

- `TestWorld` = `AppWorld` but with `db: TestDb`, `http: FakeHttp`, etc.
- `ProdWorld` = real implementations.
- `NoDbWorld` = `AppWorld` minus `db` (your `Omit` idea, but for capabilities).

Macros:

- **`@world_config`** to build `AppWorld` from components.
- **`@derive(WorldSubset)`** to auto‑derive `NoDbWorld` as `Omit<AppWorld, { db }>`.
- **`@inject`** to wire up a particular `World` for main, tests, CLI, etc.

This is essentially **capability‑based security + dependency injection + effect handlers**, unified into a single macro‑driven framework.

---

## Summary: The “play nicely with macros” pattern

For each of these built‑in types, the macro story is the same:

1. **Types describe the structure / constraints**  
   (`Ctx<Name, Row>`, `Eff<Row>`, `View<T, Fields>`, `Api<Method, Path, Req, Res>`, `World<Row>`).

2. **Macros implement the boilerplate**  
   - `@derive(Capability)`, `@derive(Effect)`, `@derive(View)`, `@api_group`, `@world_config`, etc.
   - Use CTE + reflection to inspect types and generate impls, handlers, lenses, schemas, etc.

3. **Library authors extend the language**  
   - They write `@custom_lint`, `@instrument`, `@mode_switch`, `@dsl`.
   - Users get domain‑specific ergonomics and error messages, not generic compiler errors.

If you design your language so that **every powerful type comes with a macro that makes it usable**, you avoid the “cool type system that’s too hard to use” trap and instead get something that feels like magic — but is still sound and fast.
