//! Headless CLI to preview mission-enhancer rendering.
//!
//! Loads the same datacore/INI the patcher does, then prints titles +
//! descriptions to stdout exactly as they'd appear in-game (with ANSI
//! color codes mapped from the SC HUD `<EMx>` markup). Useful for
//! debugging from a shell or AI agent — no game restart needed.
//!
//! Usage:
//!
//! ```text
//! cargo run --bin preview_cli                                # all pools (huge)
//! cargo run --bin preview_cli -- --filter "Sweep and Clear"  # title contains substring (case-insensitive)
//! cargo run --bin preview_cli -- --key "RAIN_collectresources_multiple_GEN_desc_01"  # exact desc key
//! cargo run --bin preview_cli -- --registries                # only the registry summary
//! cargo run --bin preview_cli -- --missing                   # pools whose key isn't in global.ini
//! cargo run --bin preview_cli -- --limit 5                   # cap output to first N pools
//! cargo run --bin preview_cli -- --no-color                  # strip ANSI codes
//! ```

use std::process::ExitCode;

use sc_langpatch_lib::modules::mission_enhancer::{
    CrimestatTagMode, DescOptions, TitleOptions,
};
use sc_langpatch_lib::preview::{self, PreviewSession};

struct Args {
    filter: Option<String>,
    key: Option<String>,
    limit: Option<usize>,
    registries_only: bool,
    missing_only: bool,
    no_color: bool,
}

impl Args {
    fn parse() -> Result<Self, String> {
        let mut a = Self {
            filter: None,
            key: None,
            limit: None,
            registries_only: false,
            missing_only: false,
            no_color: false,
        };
        let mut iter = std::env::args().skip(1);
        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--filter" => a.filter = Some(iter.next().ok_or("--filter needs a value")?),
                "--key" => a.key = Some(iter.next().ok_or("--key needs a value")?),
                "--limit" => {
                    a.limit = Some(
                        iter.next()
                            .ok_or("--limit needs a value")?
                            .parse()
                            .map_err(|_| "--limit must be an integer")?,
                    );
                }
                "--registries" => a.registries_only = true,
                "--missing" => a.missing_only = true,
                "--no-color" => a.no_color = true,
                "-h" | "--help" => return Err(usage()),
                other => return Err(format!("unknown argument: {other}")),
            }
        }
        Ok(a)
    }
}

fn usage() -> String {
    "preview_cli — render mission-enhancer output to stdout\n\n\
     Flags:\n\
     \x20 --filter <s>     Only pools whose title or key contains <s> (case-insensitive)\n\
     \x20 --key <s>        Only the pool with exact description key = <s>\n\
     \x20 --limit <n>      Cap output to first <n> pools\n\
     \x20 --registries     Print registry summary and exit\n\
     \x20 --missing        Print pools whose key isn't in global.ini\n\
     \x20 --no-color       Strip ANSI color codes\n"
        .into()
}

fn main() -> ExitCode {
    let args = match Args::parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    eprintln!("Loading datacore...");
    let session = match PreviewSession::load() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to load: {e:#}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!(
        "Loaded {} v{} at {}",
        session.install.channel,
        session.install.short_version(),
        session.install.root.display(),
    );

    print_registries(&session);

    if args.registries_only {
        return ExitCode::SUCCESS;
    }

    if args.missing_only {
        print_missing(&session);
        return ExitCode::SUCCESS;
    }

    let title_opts = default_title_opts();
    let desc_opts = default_desc_opts();
    let mut printed = 0usize;

    for (desc_key, ids) in session.description_pools() {
        if let Some(ref k) = args.key
            && desc_key != *k
        {
            continue;
        }
        if let Some(ref filter) = args.filter
            && !pool_matches_filter(&session, &desc_key, ids, filter)
        {
            continue;
        }
        if let Some(limit) = args.limit
            && printed >= limit
        {
            break;
        }

        let Some(rendered_desc) = session.render_description(&desc_key, ids, desc_opts) else {
            continue;
        };

        // Find the matching title pool when one exists. A description
        // can be shared across missions whose titles aren't aligned;
        // we use the first member's title as a representative.
        let title_block = title_for_pool(&session, ids, title_opts);

        print_pool_block(&session, &desc_key, ids, &title_block, &rendered_desc, !args.no_color);
        printed += 1;
    }

    if printed == 0 {
        eprintln!("(no pools matched)");
    } else {
        eprintln!("Rendered {printed} pool(s).");
    }
    ExitCode::SUCCESS
}

fn print_registries(session: &PreviewSession) {
    let r = session.registry_summary();
    eprintln!(
        "Registries: manufacturers={}, ships={}, blueprint_pools={} ({} items, {} with names), localities={}, missions={}",
        r.manufacturers,
        r.ships,
        r.blueprint_pools,
        r.blueprint_items,
        r.blueprint_items_with_name,
        r.localities,
        r.missions,
    );
    if r.ships == 0 || r.blueprint_pools == 0 {
        eprintln!(
            "  WARNING: a required registry is empty. \
             The sc-extract feature flags (entityclassdefinition / contracts / servicebeacon) \
             may not be propagating into this build."
        );
    }
}

fn print_missing(session: &PreviewSession) {
    let mut missing_titles = 0usize;
    let mut missing_descs = 0usize;
    for (key, _ids) in session.title_pools() {
        if !session.ini.contains_key(&key) {
            println!("title  {key}");
            missing_titles += 1;
        }
    }
    for (key, _ids) in session.description_pools() {
        if !session.ini.contains_key(&key) {
            println!("desc   {key}");
            missing_descs += 1;
        }
    }
    eprintln!(
        "Missing keys: {missing_titles} title pool(s), {missing_descs} description pool(s)"
    );
}

fn pool_matches_filter(
    session: &PreviewSession,
    desc_key: &str,
    ids: &[sc_extract::Guid],
    filter: &str,
) -> bool {
    let needle = filter.to_lowercase();
    if desc_key.to_lowercase().contains(&needle) {
        return true;
    }
    // Also match against the title text and debug names of members,
    // so a search for "Sweep and Clear" hits the actual displayed title.
    if let Some(head) = ids.first().and_then(|id| session.index.get(*id)) {
        if let Some(t) = head.title(&session.locale)
            && t.to_lowercase().contains(&needle)
        {
            return true;
        }
        if head.debug_name.to_lowercase().contains(&needle) {
            return true;
        }
    }
    false
}

fn title_for_pool(
    session: &PreviewSession,
    ids: &[sc_extract::Guid],
    opts: TitleOptions,
) -> Option<String> {
    let head = session.index.get(*ids.first()?)?;
    let key = head.title_key.as_ref()?.stripped().to_string();
    // The title pool that owns this key may have different membership
    // than the description pool; render against the title pool's
    // members for accurate tagging.
    let title_ids = session.index.pools.title_key.get(head.title_key.as_ref()?)?;
    session.render_title(&key, title_ids, opts)
}

fn print_pool_block(
    session: &PreviewSession,
    desc_key: &str,
    ids: &[sc_extract::Guid],
    title_block: &Option<String>,
    rendered_desc: &str,
    use_color: bool,
) {
    let translate = |s: &str| -> String {
        if use_color {
            preview::translate_to_ansi(s)
        } else {
            // Strip both newline literals and color tags for plain output.
            let with_newlines = s.replace("\\n", "\n");
            strip_color_tags(&with_newlines)
        }
    };

    println!("{}", "═".repeat(78));
    println!("desc_key: {desc_key}  (members: {})", ids.len());
    if let Some(head) = ids.first().and_then(|id| session.index.get(*id)) {
        println!("first member: {} [{}]", head.debug_name, format_guid(&head.id));
    }
    println!();
    if let Some(title) = title_block {
        println!("{}", translate(title));
        println!();
    }
    println!("{}", translate(rendered_desc));
    println!();
}

fn strip_color_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        if c == '<' {
            in_tag = true;
            continue;
        }
        if c == '>' && in_tag {
            in_tag = false;
            continue;
        }
        if !in_tag {
            out.push(c);
        }
    }
    out
}

fn format_guid(g: &sc_extract::Guid) -> String {
    let bytes = g.as_bytes();
    let mut s = String::with_capacity(36);
    for (i, b) in bytes.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            s.push('-');
        }
        s.push_str(&format!("{b:02x}"));
    }
    s
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
        // CLI keeps stdout clean for piping — diagnostics belong in
        // the patcher run.
        diagnostics: false,
    }
}
