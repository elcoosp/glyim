use crate::AnalysisDatabase;
use lsp_types::*;

pub fn provide_code_actions(
    _db: &AnalysisDatabase,
    _params: &CodeActionParams,
) -> Option<Vec<CodeActionOrCommand>> {
    // For now, return no actions.
    None
}
