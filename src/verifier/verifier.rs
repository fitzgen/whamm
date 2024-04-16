use crate::parser::types::{Whamm, WhammVisitor};
use crate::verifier::builder_visitor::SymbolTableBuilder;
use crate::verifier::types::SymbolTable;

pub fn verify(ast: &Whamm) -> SymbolTable {
    let table = build_symbol_table(&ast);

    // TODO do typechecking!
    return table;
}

// ================
// = SYMBOL TABLE =
// ================

fn build_symbol_table(ast: &Whamm) -> SymbolTable {
    let mut visitor = SymbolTableBuilder::new();
    visitor.visit_whamm(ast);
    visitor.table
}
