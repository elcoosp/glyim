#[allow(unused_imports, dead_code)]
use crate::common::*;
#[cfg(test)]
mod arithmetic_proptests {
    use glyim_cli::pipeline;

    proptest! {
        #[test]
        fn add(a in 0i64..100, b in 0i64..100) {
            let src = format!("main = () => {} + {}", a, b);
            let result = pipeline::run_jit(&src).unwrap();
            prop_assert_eq!(result as i64, a + b);
        }

        #[test]
        fn sub(a in 0i64..100, b in 0i64..100) {
            let src = format!("main = () => {} - {}", a, b);
            let result = pipeline::run_jit(&src).unwrap();
            prop_assert_eq!(result as i64, a - b);
        }

        #[test]
        fn mul(a in 0i64..20, b in 0i64..20) {
            let src = format!("main = () => {} * {}", a, b);
            let result = pipeline::run_jit(&src).unwrap();
            prop_assert_eq!(result as i64, a * b);
        }

        #[test]
        fn div(a in 1i64..100, b in 1i64..100) {
            let src = format!("main = () => {} / {}", a, b);
            let result = pipeline::run_jit(&src).unwrap();
            prop_assert_eq!(result as i64, a / b);
        }
    }
}
