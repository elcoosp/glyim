use crate::session::state::{SessionState, StreamStatus};
use comfy_table::{presets::UTF8_FULL, Cell, Color, Table};

/// `render_status_table`.
#[must_use]
pub fn render_status_table(sessions: &[SessionState]) -> String {
    if sessions.is_empty() {
        return "No active sessions.".to_string();
    }
    let mut table = Table::new();
    table.load_style(UTF8_FULL);
    table.set_header(vec![
        "Stream",
        "Provider",
        "Status",
        "Turn",
        "Fixes",
        "Commits",
        "Last Activity",
    ]);
    for s in sessions {
        let color = match s.status {
            StreamStatus::Complete => Color::Green,
            StreamStatus::Error => Color::Red,
            StreamStatus::Paused => Color::Yellow,
            _ => Color::White,
        };
        table.add_row(vec![
            Cell::new(&s.stream_id),
            Cell::new(&s.provider_id),
            Cell::new(format!("{:?}", s.status)).fg(color),
            Cell::new(s.turn),
            Cell::new(s.fix_round),
            Cell::new(s.commits),
            Cell::new(s.last_activity.format("%H:%M:%S")),
        ]);
    }
    table.to_string()
}

/// `render_wave_summary`.
#[must_use]
pub fn render_wave_summary(sessions: &[SessionState]) -> String {
    if sessions.is_empty() {
        return "No sessions in wave.".to_string();
    }
    let total_turns: u32 = sessions.iter().map(|s| s.turn).sum();
    let total_commits: u32 = sessions.iter().map(|s| s.commits).sum();
    let completed = sessions
        .iter()
        .filter(|s| s.status == StreamStatus::Complete)
        .count();
    format!(
        "Summary: {completed}/{} complete, {} total turns, {} total commits",
        sessions.len(),
        total_turns,
        total_commits
    )
}
