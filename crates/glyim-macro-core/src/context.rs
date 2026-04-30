use glyim_interner::Symbol;

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: Symbol,
    pub ty: Symbol,
}

pub trait MacroContext {
    fn trait_is_implemented(&self, trait_name: Symbol, for_type: Symbol) -> bool;
    fn get_fields(&self, struct_name: Symbol) -> Vec<Field>;
    fn get_type_params(&self, struct_name: Symbol) -> Vec<Symbol>;
}
