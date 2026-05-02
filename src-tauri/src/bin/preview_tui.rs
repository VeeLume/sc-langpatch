//! Interactive TUI for previewing mission-enhancer rendering.
//!
//! Two-pane browser: filterable description-pool list on the left,
//! rendered output (title with tags + description with patch suffix)
//! on the right. The same code path the patcher uses produces the
//! text — no game restart needed to validate a fix.
//!
//! Bindings:
//! - Type into filter to narrow the list (matches key, title, debug name)
//! - ↑/↓ or j/k    — move list selection
//! - PgUp/PgDn     — scroll detail pane
//! - Tab           — toggle list/detail focus
//! - Ctrl+Q / Esc  — quit

use std::process::ExitCode;

use anyhow::Context as _;
use sc_extract::Guid;
use sc_langpatch_lib::formatter_helpers::Color as MarkupColor;
use sc_langpatch_lib::modules::mission_enhancer::{
    CrimestatTagMode, DescOptions, TitleOptions,
};
use sc_langpatch_lib::preview::{self, PreviewSession};
use slt::{
    Border, Color, Context, KeyCode, KeyModifiers, ListState, RunConfig, ScrollState,
    TextInputState,
};

fn main() -> ExitCode {
    eprintln!("preview_tui: loading datacore (~5–10s)...");
    let session = match PreviewSession::load() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to load: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!(
        "preview_tui: loaded {} v{}",
        session.install.channel,
        session.install.short_version()
    );

    let pools: Vec<PoolEntry> = build_pool_entries(&session);

    let mut state = AppState {
        filter: TextInputState::with_placeholder("filter (key / title / debug name)"),
        list: ListState::new(Vec::<String>::new()),
        detail_scroll: ScrollState::new(),
        pools,
        title_opts: default_title_opts(),
        desc_opts: default_desc_opts(),
    };

    let render_pump = |ui: &mut Context| draw(ui, &mut state, &session);

    if let Err(e) = slt::run_with(RunConfig::default().mouse(true), render_pump)
        .context("run TUI")
    {
        eprintln!("TUI error: {e:#}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

// ── App state ──────────────────────────────────────────────────────────────

struct AppState {
    filter: TextInputState,
    list: ListState,
    detail_scroll: ScrollState,
    pools: Vec<PoolEntry>,
    title_opts: TitleOptions,
    desc_opts: DescOptions,
}

/// One pool's pre-extracted searchable text. Avoids re-walking
/// `MissionIndex` on every keystroke.
struct PoolEntry {
    desc_key: String,
    /// Pool member ids (slice into `MissionIndex`).
    ids: Vec<Guid>,
    /// First member's title (or debug name fallback) — used as the
    /// list label and for substring search.
    label: String,
    /// First member's debug name — also searchable.
    debug_name: String,
    member_count: usize,
}

fn build_pool_entries(session: &PreviewSession) -> Vec<PoolEntry> {
    let mut out: Vec<PoolEntry> = Vec::new();
    for (desc_key, ids) in session.description_pools() {
        let head = ids.first().and_then(|id| session.index.get(*id));
        let label = head
            .and_then(|m| m.title.clone())
            .or_else(|| head.map(|m| m.debug_name.clone()))
            .unwrap_or_else(|| desc_key.clone());
        let debug_name = head.map(|m| m.debug_name.clone()).unwrap_or_default();
        out.push(PoolEntry {
            desc_key,
            ids: ids.to_vec(),
            label,
            debug_name,
            member_count: ids.len(),
        });
    }
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out
}

// ── Rendering ──────────────────────────────────────────────────────────────

fn draw(ui: &mut Context, state: &mut AppState, session: &PreviewSession) {
    handle_quit(ui);

    let filter = state.filter.value.to_lowercase();
    let filtered: Vec<usize> = state
        .pools
        .iter()
        .enumerate()
        .filter(|(_, p)| matches(&filter, p))
        .map(|(i, _)| i)
        .collect();

    state.list.selected = state
        .list
        .selected
        .min(filtered.len().saturating_sub(1).max(0));

    let _ = ui.container().grow(1).col(|ui| {
        // Header: title + counts
        let _ = ui.row(|ui| {
            ui.text("Mission Preview").bold().fg(Color::Cyan);
            ui.spacer();
            let r = session.registry_summary();
            ui.text(format!(
                "ships={} bp_pools={} ({}/{} named) localities={} missions={}",
                r.ships,
                r.blueprint_pools,
                r.blueprint_items_with_name,
                r.blueprint_items,
                r.localities,
                r.missions,
            ))
            .dim();
        });

        // Filter input
        let _ = ui.container().h(3).col(|ui| {
            let _ = ui.text_input(&mut state.filter);
        });
        ui.separator();

        // Body — list + detail
        let _ = ui.container().grow(1).row(|ui| {
            // Left: pool list
            let _ = ui
                .bordered(Border::Rounded)
                .title(format!(
                    "Pools ({} matching / {} total)",
                    filtered.len(),
                    state.pools.len()
                ))
                .p(1)
                .grow(1)
                .col(|ui| {
                    if filtered.is_empty() {
                        ui.text("(no matches)").dim();
                    } else {
                        let items: Vec<String> = filtered
                            .iter()
                            .map(|&i| format_list_item(&state.pools[i]))
                            .collect();
                        state.list.set_items(items.clone());
                        let visible = list_visible_rows(ui);
                        scroll_list(ui, &mut state.list, &items, visible);
                    }
                });

            // Right: rendered detail
            let _ = ui
                .bordered(Border::Rounded)
                .title("Rendered")
                .p(1)
                .grow(2)
                .col(|ui| match filtered.get(state.list.selected).copied() {
                    Some(idx) => render_detail(ui, state, session, idx),
                    None => {
                        ui.text("(nothing selected)").dim();
                    }
                });
        });

        // Footer: bindings hint
        ui.separator();
        let _ = ui.row(|ui| {
            ui.text("↑/↓ list   ").dim();
            ui.text("PgUp/PgDn detail   ").dim();
            ui.text("type to filter   ").dim();
            ui.text("Esc / Ctrl+Q quit").dim();
        });
    });
}

fn render_detail(ui: &mut Context, state: &mut AppState, session: &PreviewSession, idx: usize) {
    let pool = &state.pools[idx];

    let _ = ui.row(|ui| {
        ui.text("desc_key: ").dim();
        ui.text(&pool.desc_key);
    });
    let _ = ui.row(|ui| {
        ui.text("members:  ").dim();
        ui.text(format!("{} ", pool.member_count));
        ui.text("first debug_name: ").dim();
        ui.text(&pool.debug_name);
    });
    ui.separator();

    let title = title_for_pool(session, &pool.ids, state.title_opts);
    let desc = session
        .render_description(&pool.desc_key, &pool.ids, state.desc_opts)
        .unwrap_or_else(|| "(desc_key not in global.ini)".to_string());

    let _ = ui
        .scrollable(&mut state.detail_scroll)
        .grow(1)
        .col(|ui| {
            if let Some(t) = title {
                render_styled_text(ui, &t);
                ui.text("");
            }
            render_styled_text(ui, &desc);
        });
}

/// Render a string containing `<EMx>` markup + `\n` literals as
/// styled superlighttui text widgets, one logical line per source
/// line. Each logical line goes through [`Context::line_wrap`] so
/// long passages wrap at word boundaries instead of getting clipped
/// at the panel border.
fn render_styled_text(ui: &mut Context, s: &str) {
    let runs = preview::parse_styled_runs(s);

    // Split colored runs into logical lines first — `line_wrap`
    // renders inline content, so newline boundaries need to become
    // separate calls.
    let mut current_line: Vec<(MarkupColor, String)> = Vec::new();
    let flush_line = |ui: &mut Context, line: &mut Vec<(MarkupColor, String)>| {
        if line.is_empty() {
            ui.text("");
            return;
        }
        // `line_wrap` reflows the inline segments at word boundaries
        // when the combined width exceeds the container, while keeping
        // each segment's color.
        let segments = std::mem::take(line);
        ui.line_wrap(|ui| {
            for (color, segment) in segments {
                ui.text(segment).fg(map_color(color));
            }
        });
    };

    for (color, text) in runs {
        // Each `\n` in `text` ends the current line.
        let mut iter = text.split('\n').peekable();
        while let Some(part) = iter.next() {
            if !part.is_empty() {
                current_line.push((color, part.to_string()));
            }
            if iter.peek().is_some() {
                flush_line(ui, &mut current_line);
            }
        }
    }
    // Final line (no trailing newline).
    flush_line(ui, &mut current_line);
}

/// Map our semantic emphasis levels onto the terminal palette so
/// the TUI roughly mirrors the in-game contracts panel: Highlight
/// → Cyan (close to SC's blue accent), Underline → Yellow (no
/// portable way to terminal-underline mid-line in SLT, fall back to
/// a distinct color), the rest → default white. The mapping is
/// approximate — exact in-game appearance still belongs in the live
/// game.
fn map_color(c: MarkupColor) -> Color {
    match c {
        MarkupColor::Plain | MarkupColor::Faint | MarkupColor::Soft => Color::White,
        MarkupColor::Underline => Color::Yellow,
        MarkupColor::Highlight => Color::Cyan,
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn matches(filter: &str, p: &PoolEntry) -> bool {
    if filter.is_empty() {
        return true;
    }
    p.desc_key.to_lowercase().contains(filter)
        || p.label.to_lowercase().contains(filter)
        || p.debug_name.to_lowercase().contains(filter)
}

fn format_list_item(p: &PoolEntry) -> String {
    let suffix = if p.member_count > 1 {
        format!(" ×{}", p.member_count)
    } else {
        String::new()
    };
    format!("{}{suffix}", truncate(&p.label, 50))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max - 1).collect();
        format!("{head}…")
    }
}

fn title_for_pool(
    session: &PreviewSession,
    ids: &[Guid],
    opts: TitleOptions,
) -> Option<String> {
    let head = session.index.get(*ids.first()?)?;
    let title_key_raw = head.title_key.as_ref()?;
    let title_pool = session.index.pools.title_key.get(title_key_raw)?;
    session.render_title(title_key_raw.stripped(), title_pool, opts)
}

fn handle_quit(ui: &mut Context) {
    if ui.key_mod('q', KeyModifiers::CONTROL) || ui.key_code(KeyCode::Esc) {
        ui.quit();
    }
}

/// Visible row budget for the scrollable list. Subtracts the
/// header / filter / footer chrome from the terminal height with a
/// small fudge factor so the "↓ N more" indicator always fits.
fn list_visible_rows(ui: &Context) -> usize {
    (ui.height() as usize).saturating_sub(12).max(5)
}

/// Selection-aware list with a viewport that always contains
/// `state.selected`. SLT's built-in `ui.list` renders every item
/// without scrolling and doesn't consume keyboard input — so the
/// cursor moves off-screen as soon as the list is taller than the
/// pane. Pattern lifted from `sc-contracts::tui` where the same
/// limitation drove the same workaround.
fn scroll_list(
    ui: &mut Context,
    state: &mut slt::ListState,
    items: &[String],
    visible: usize,
) {
    let total = items.len();
    if total == 0 {
        return;
    }
    if state.selected >= total {
        state.selected = total - 1;
    }
    let focused = ui.register_focusable();
    if focused {
        if ui.consume_key('j') || ui.consume_key_code(KeyCode::Down) {
            state.selected = (state.selected + 1).min(total - 1);
        }
        if ui.consume_key('k') || ui.consume_key_code(KeyCode::Up) {
            state.selected = state.selected.saturating_sub(1);
        }
        if ui.consume_key_code(KeyCode::PageDown) {
            state.selected = (state.selected + visible).min(total - 1);
        }
        if ui.consume_key_code(KeyCode::PageUp) {
            state.selected = state.selected.saturating_sub(visible);
        }
        if ui.consume_key_code(KeyCode::Home) {
            state.selected = 0;
        }
        if ui.consume_key_code(KeyCode::End) {
            state.selected = total - 1;
        }
    }

    // Reserve a row for each indicator so they don't push items out.
    let body_rows = visible.saturating_sub(2).max(1);
    let start = if state.selected >= body_rows {
        state.selected + 1 - body_rows
    } else {
        0
    };
    let end = (start + body_rows).min(total);

    if start > 0 {
        ui.text(format!("  ↑ {start} more")).dim();
    }
    for (idx, text) in items.iter().enumerate().take(end).skip(start) {
        if idx == state.selected {
            ui.text(format!("► {text}"))
                .bold()
                .fg(if focused { Color::Cyan } else { Color::White });
        } else {
            ui.text(format!("  {text}"));
        }
    }
    let remaining = total.saturating_sub(end);
    if remaining > 0 {
        ui.text(format!("  ↓ {remaining} more")).dim();
    }
}

fn default_title_opts() -> TitleOptions {
    TitleOptions {
        blueprint: true,
        solo: true,
        once: true,
        illegal: true,
        crimestat: CrimestatTagMode::from_str("colored"),
    }
}

fn default_desc_opts() -> DescOptions {
    DescOptions {
        blueprint_list: true,
        mission_info: true,
        ship_encounters: true,
        cargo_info: true,
        region_info: true,
        // TUI re-renders ~30 fps — the per-pool fallback eprintln
        // would flood stderr and bleed across the screen on every
        // frame the same pool stays selected.
        diagnostics: false,
    }
}
