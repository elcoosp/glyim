use clap::Subcommand;
use reqwest::Client;
use serde_json;
use uuid::Uuid;
const SYSTEM_PROMPT: &str = r#"You are an AI assistant that implements code changes.
Your entire response must be a single Markdown code block with the language "glyim-ops".

Inside the block, you can use these directives:

- `::WRITE <path>`
  Write the entire content of a new or existing file. Follow with the file content, then `::END` on a new line.
- `::REPLACE <path>`
  Replace a specific section of a file. Use `---FIND---` on its own line, then the exact text to replace, then `---REPLACE---` on its own line, then the new text, then `::END`.
- `::DELETE <path>`
  Delete a file.
- `::COMMIT "message"`
  **MUST** be included after every set of file modifications. The commit message should describe the changes.
- `::DONE`
  Signal that the task is complete and the code is ready for review.
- `::INCOMPLETE`
  Signal that you need another turn (e.g., response cut off).
- `::APPROVED`
  Approve a self‑review (used after `::DONE`).

**IMPORTANT**: After any `::WRITE`, `::REPLACE`, or `::DELETE`, you MUST include a `::COMMIT` line before closing the code block. Otherwise, the system will wait indefinitely.

Example response for adding a function:

```glyim-ops
::WRITE src/lib.rs
fn multiply(a: i32, b: i32) -> i32 {
    a * b
}
::END
::COMMIT "Add multiply function"
Do not output any text outside the code block. Use Rust unless the user asks for another language."#;

#[derive(Subcommand)]
/// `SessionCommands`.
pub enum SessionCommands {
    /// Start a new session with a provider
    Start {
        /// Provider ID (deepseek, gemini, grok, etc.)
        #[arg(short = 'P', long, default_value = "deepseek")]
        provider: String,
        /// Prompt to send
        #[arg(
            short = 'p',
            long,
            default_value = "Write a simple Rust function that adds two numbers"
        )]
        prompt: String,
        /// Optional session ID (auto-generated if not provided)
        #[arg(short, long)]
        session_id: Option<String>,
    },
}

/// `handle_session_command`.
pub async fn handle_session_command(cmd: SessionCommands) -> Result<(), anyhow::Error> {
    match cmd {
        SessionCommands::Start {
            provider,
            prompt,
            session_id,
        } => {
            let session_id = session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
            let client = Client::new();
            let resp = client
                .post("http://127.0.0.1:8421/session/start")
                .json(&serde_json::json!({
                    "provider": provider,
                    "prompt": prompt,
                    "session_id": session_id,
                    "system_prompt": SYSTEM_PROMPT,
                }))
                .send()
                .await?;
            if resp.status().is_success() {
                println!("Session start sent.");
            } else {
                eprintln!("Failed: {}", resp.text().await?);
            }
            Ok(())
        }
    }
}
