use glyim_interner::Symbol;
use std::collections::{HashMap, HashSet};

pub struct NameDependencyTable {
    definitions: HashMap<Symbol, HashSet<Symbol>>,
    references: HashMap<Symbol, HashSet<Symbol>>,
    dependents: HashMap<Symbol, HashSet<Symbol>>,
}

static EMPTY: HashSet<Symbol> = HashSet::new();

impl NameDependencyTable {
    pub fn new() -> Self {
        Self {
            definitions: HashMap::new(),
            references: HashMap::new(),
            dependents: HashMap::new(),
        }
    }

    pub fn add_definition(&mut self, item: Symbol, name: Symbol) {
        self.definitions.entry(item).or_default().insert(name);
    }

    pub fn add_reference(&mut self, item: Symbol, name: Symbol) {
        self.references.entry(item).or_default().insert(name);
        self.dependents.entry(name).or_default().insert(item);
    }

    pub fn definitions_for_sym(&self, item: Symbol) -> &HashSet<Symbol> {
        self.definitions.get(&item).unwrap_or(&EMPTY)
    }

    pub fn references_for_sym(&self, item: Symbol) -> &HashSet<Symbol> {
        self.references.get(&item).unwrap_or(&EMPTY)
    }

    pub fn direct_dependents(&self, name: Symbol) -> &HashSet<Symbol> {
        self.dependents.get(&name).unwrap_or(&EMPTY)
    }

    pub fn transitive_dependents(&self, changed: &[Symbol]) -> HashSet<Symbol> {
        let mut affected = HashSet::new();
        let mut queue: Vec<Symbol> = changed.to_vec();
        while let Some(name) = queue.pop() {
            if let Some(deps) = self.dependents.get(&name) {
                for &dep in deps {
                    if affected.insert(dep) {
                        if let Some(defs) = self.definitions.get(&dep) {
                            for &def_name in defs {
                                if !changed.contains(&def_name) && !affected.contains(&def_name)
                                {
                                    if let Some(sub_deps) = self.dependents.get(&def_name) {
                                        for &sub_dep in sub_deps {
                                            if affected.insert(sub_dep) {
                                                queue.push(sub_dep);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        queue.push(dep);
                    }
                }
            }
        }
        affected
    }
}

impl Default for NameDependencyTable {
    fn default() -> Self {
        Self::new()
    }
}
