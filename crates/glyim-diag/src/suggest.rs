use glyim_interner::Interner;

/// Suggest similar symbols for an undefined name, using Levenshtein edit distance.
pub fn suggest_similar(
    name: &str,
    interner: &Interner,
    max_suggestions: usize,
) -> Vec<String> {
    let mut candidates: Vec<(usize, String)> = interner
        .all_symbols()
        .into_iter()
        .filter_map(|sym| {
            let dist = levenshtein_distance(name, sym);
            if dist <= max_edit_distance(name) {
                Some((dist, sym.to_string()))
            } else {
                None
            }
        })
        .collect();

    candidates.sort_by_key(|(dist, _)| *dist);
    candidates.truncate(max_suggestions);
    candidates.into_iter().map(|(_, name)| name).collect()
}

fn max_edit_distance(name: &str) -> usize {
    match name.len() {
        0..=3 => 1,
        4..=6 => 2,
        _ => 3,
    }
}

fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_len = a.chars().count();
    let b_len = b.chars().count();
    let mut matrix = vec![vec![0; b_len + 1]; a_len + 1];

    for (i, row) in matrix.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, entry) in matrix[0].iter_mut().enumerate() {
        *entry = j;
    }

    for (i, a_char) in a.chars().enumerate() {
        for (j, b_char) in b.chars().enumerate() {
            let cost = if a_char == b_char { 0 } else { 1 };
            matrix[i + 1][j + 1] = (matrix[i][j + 1] + 1)
                .min(matrix[i + 1][j] + 1)
                .min(matrix[i][j] + cost);
        }
    }

    matrix[a_len][b_len]
}
