use glyim_testr::config::TestConfig;
use std::path::PathBuf;

pub fn cmd_test(
    input: PathBuf,
    ignore: bool,
    filter: Option<String>,
    nocapture: bool,
    watch: bool,
    optimize_check: bool,
    _remote_cache: Option<String>,
    coverage: bool,
    mutate: bool,
    mutation_score: Option<f64>,
    mutation_operators: Option<String>,
    max_mutants: Option<usize>,
    concurrent_mutants: Option<usize>,
    mutation_report: Option<PathBuf>,
    coverage_mode: Option<String>,
) -> i32 {
    let source = match std::fs::read_to_string(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error reading {}: {}", input.display(), e);
            return 1;
        }
    };

    if mutate {
        // ----------------------------------------------------
        // Mutation testing mode
        // ----------------------------------------------------
        let config = glyim_mutant::config::MutationConfig {
            operators: parse_mutation_operators(mutation_operators.as_deref()),
            max_mutations_per_fn: max_mutants.unwrap_or(50),
            ..Default::default()
        };

        let mut runner = glyim_mutant::runner::MutationRunner::new(config);
        // Collect test names from source (simple collector, no dependency on testr)
        let test_names = collect_test_names(&source);
        let results = runner.run(&source, &test_names);

        // Build and print report
        let total = results.len();
        let killed = results.iter().filter(|(_, killed)| *killed).count();
        let survived = results.iter().filter(|(_, killed)| !*killed).count();
        let score = if total > 0 {
            (killed as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        eprintln!("Mutation Testing Report for {}", input.display());
        eprintln!("═══════════════════════════════════");
        eprintln!("Total mutations:    {}", total);
        eprintln!("  Killed:           {}", killed);
        eprintln!("  Survived:         {}", survived);
        eprintln!("Mutation Score:     {:.1}%", score);

        // Show survived mutants
        if survived > 0 {
            eprintln!("\nSurvived Mutants:");
            for (mutation, _) in &results {
                if !results.iter().any(|(m, k)| m.id == mutation.id && *k) {
                    eprintln!("  {}. {}::{}", mutation.id, mutation.function_name,
                        format!("{:?}", mutation.operator));
                }
            }
        }

        // Save JSON report if requested
        if let Some(report_path) = mutation_report {
            let json = serde_json::json!({
                "source_file": input.to_string_lossy(),
                "total_mutations": total,
                "killed": killed,
                "survived": survived,
                "score": score,
                "mutations": results.iter().map(|(m, killed)| {
                    serde_json::json!({
                        "id": m.id,
                        "function_name": m.function_name,
                        "operator": format!("{:?}", m.operator),
                        "killed": killed,
                    })
                }).collect::<Vec<_>>()
            });
            if let Err(e) = std::fs::write(&report_path, serde_json::to_string_pretty(&json).unwrap()) {
                eprintln!("warning: failed to write mutation report: {e}");
            }
        }

        // Check score threshold
        if let Some(threshold) = mutation_score {
            if score < threshold {
                eprintln!("Mutation score {:.1}% is below threshold {:.1}%", score, threshold);
                return 1;
            }
        }

        return if survived > 0 { 1 } else { 0 };
    }

    // ----------------------------------------------------
    // Regular test mode (existing logic)
    // ----------------------------------------------------
    let config = TestConfig {
        filter,
        include_ignored: ignore,
        nocapture,
        watch,
        optimize_check,
        ..Default::default()
    };

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error creating runtime: {}", e);
            return 1;
        }
    };

    if watch {
        rt.block_on(glyim_testr::run_watch(&input, &config));
        0
    } else {
        let results = rt.block_on(glyim_testr::run_tests(&source, &config));
        let failed = results
            .iter()
            .filter(|r| matches!(r.outcome, glyim_testr::types::TestOutcome::Failed { .. }))
            .count();
        if failed > 0 { 1 } else { 0 }
    }
}

fn parse_mutation_operators(spec: Option<&str>) -> Vec<glyim_mutant::config::MutationOperator> {
    use glyim_mutant::config::MutationOperator;
    if let Some(spec) = spec {
        spec.split(',')
            .filter_map(|s| match s.trim() {
                "plus-to-minus" => Some(MutationOperator::ArithmeticPlusToMinus),
                "minus-to-plus" => Some(MutationOperator::ArithmeticMinusToPlus),
                "mul-to-div" => Some(MutationOperator::ArithmeticMulToDiv),
                "div-to-mul" => Some(MutationOperator::ArithmeticDivToMul),
                "eq-to-neq" => Some(MutationOperator::CompareEqualToNotEqual),
                "neq-to-eq" => Some(MutationOperator::CompareNotEqualToEqual),
                "and-to-or" => Some(MutationOperator::BooleanAndToOr),
                "or-to-and" => Some(MutationOperator::BooleanOrToAnd),
                "not-elim" => Some(MutationOperator::BooleanNotElimination),
                "const-zero" => Some(MutationOperator::ConstantZero),
                "stmt-del" => Some(MutationOperator::StatementDeletion),
                "cond-flip" => Some(MutationOperator::ConditionalFlip),
                _ => None,
            })
            .collect()
    } else {
        glyim_mutant::config::MutationConfig::default().operators
    }
}

fn collect_test_names(source: &str) -> Vec<String> {
    let parse_out = glyim_parse::parse(source);
    let mut names = Vec::new();
    for item in &parse_out.ast.items {
        if let glyim_parse::Item::FnDef { name, attrs, .. } = item {
            if attrs.iter().any(|a| a.name == "test") {
                names.push(parse_out.interner.resolve(*name).to_string());
            }
        }
    }
    names
}
