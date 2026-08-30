//! CLI argv surface (PORT-CONTRACT.d/01).
//!
//! Parity scope: exit codes, stdout bytes, and the final
//! `<prog>: error: <msg>` stderr line. Usage/help BODY text is out of scope
//! (decision 9) — stdout stays byte-identical (empty on errors).

use crate::commands::{
    cmd_corpus_digest, cmd_corpus_explain, cmd_corpus_status, cmd_coverage, cmd_decisions_for,
    cmd_diagnose, cmd_diff, cmd_doctor,
    cmd_eval, cmd_export, cmd_find, cmd_gate, cmd_herald, cmd_hook, cmd_improve, cmd_index,
    cmd_init, cmd_inspect, cmd_mcp_stats, cmd_migrate, cmd_new, cmd_portfolio, cmd_quickstart,
    cmd_relationships, cmd_rename, cmd_resolve, cmd_retrieve, cmd_review, cmd_schema, cmd_sentry,
    cmd_skill, cmd_stats, cmd_telemetry, cmd_templates, cmd_usage, cmd_validate,
    CorpusDigestArgs, CorpusExplainArgs, CorpusStatusArgs, CoverageArgs, DecisionsForArgs,
    DiagnoseArgs, DiffArgs, DoctorArgs, EvalArgs, ExportArgs, FindArgs, GateArgs, HeraldArgs,
    HookArgs, ImproveArgs, IndexArgs, InitArgs, InspectArgs, McpStatsArgs, MigrateArgs, NewArgs,
    PortfolioArgs, QuickstartArgs, RelationshipsArgs, RenameArgs, ResolveArgs, RetrieveArgs,
    ReviewArgs, SchemaArgs, SentryArgs, SkillArgs, StatsArgs, TelemetryArgs, TemplatesArgs,
    UsageArgs, ValidateArgs, WatchkeeperArgs,
};
use crate::commands::cmd_watchkeeper;
use crate::output::rac_version;

/// Root subcommand table, in argparse declaration order (the order the
/// `invalid choice` message quotes).
const SUBCOMMANDS: [&str; 32] = [
    "validate",
    "diff",
    "stats",
    "ingest",
    "inspect",
    "improve",
    "schema",
    "relationships",
    "rename",
    "review",
    "doctor",
    "coverage",
    "gate",
    "watchkeeper",
    "portfolio",
    "index",
    "export",
    "mcp",
    "mcp-stats",
    "telemetry",
    "usage",
    "new",
    "templates",
    "init",
    "quickstart",
    "resolve",
    "find",
    "decisions-for",
    "eval",
    "migrate",
    "skill",
    "hook",
];

fn version_line() -> String {
    format!("decided {}", rac_version())
}

fn print_stdout(text: &str) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(text.as_bytes());
    let _ = out.write_all(b"\n");
    let _ = out.flush();
}

/// The ADR-046 recorder gate: the oracle's `cli.main` computes the command
/// name only AFTER `parse_args` returns, so argparse-level exits (parse
/// errors, `--version`/`-h` actions) never record a usage event, while
/// dispatched commands record `ok`/`error` from the exit code. Every
/// parse-level early return in this module raises this flag.
static PARSE_LEVEL_EXIT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn skip_usage_record() {
    PARSE_LEVEL_EXIT.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// argparse-style error: usage to stderr, final `<prog>: error: <msg>` line,
/// exit 2. The usage body is out of parity scope; only the last line is
/// contract-shaped.
fn argparse_error(prog: &str, message: &str) -> u8 {
    skip_usage_record();
    eprintln!("usage: {prog} ...");
    eprintln!("{prog}: error: {message}");
    2
}

/// Leftover-token rejection; argparse reports these against the root prog
/// (`decided`), not the subcommand.
fn unrecognized(extras: &[String]) -> u8 {
    argparse_error(
        "decided",
        &format!("unrecognized arguments: {}", extras.join(" ")),
    )
}

fn invalid_choice_message(token: &str) -> String {
    let choices = SUBCOMMANDS
        .iter()
        .map(|s| format!("'{s}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("argument command: invalid choice: '{token}' (choose from {choices})")
}

/// Dispatch plus the ADR-046 usage recorder: one content-free event per
/// dispatched command, gated by recorded consent, silent-fail, after the
/// command completes (never before, so `telemetry on` records itself under
/// its own freshly-written consent, exactly like the oracle).
pub fn run(args: &[String]) -> u8 {
    let start = std::time::Instant::now();
    let code = run_dispatch(args);
    if !PARSE_LEVEL_EXIT.load(std::sync::atomic::Ordering::Relaxed) {
        if let Some(command) = args.first().filter(|a| !a.starts_with('-')) {
            let outcome = if code == 0 {
                crate::usage::OUTCOME_OK
            } else {
                crate::usage::OUTCOME_ERROR
            };
            crate::usage::record_command(
                command,
                outcome,
                start.elapsed().as_millis() as i64,
            );
        }
    }
    code
}

fn run_dispatch(args: &[String]) -> u8 {
    let mut it = args.iter();
    let first = match it.next() {
        None => return argparse_error("decided", "the following arguments are required: command"),
        Some(a) if a == "--version" => {
            skip_usage_record();
            print_stdout(&version_line());
            return 0;
        }
        Some(a) if a == "-h" || a == "--help" => {
            // Help body is out of parity scope; emit a stub to stdout.
            skip_usage_record();
            print_stdout("usage: decided [-h] [--version] <command> ...");
            return 0;
        }
        Some(a) => a,
    };

    if first.starts_with('-') {
        return argparse_error("decided", &format!("unrecognized arguments: {first}"));
    }
    // Native-only additions dispatch but are deliberately NOT in SUBCOMMANDS:
    // the retired Python oracle's `invalid choice` bytes remain pinned by the
    // bounded compatibility suite.
    if !matches!(
        first.as_str(),
        "retrieve" | "sentry" | "herald" | "diagnose" | "corpus"
    )
        && !SUBCOMMANDS.contains(&first.as_str())
    {
        return argparse_error("decided", &invalid_choice_message(first));
    }

    let rest: Vec<&String> = it.collect();

    // `--version` short-circuits on every subcommand (version_parent) —
    // EXCEPT where an earlier argv token can error first at its own
    // position: choice-validated positionals (telemetry, skill, hook,
    // migrate's target), a choice-validated option value (hook --style,
    // init --ticketing/--profile), an immediate mutex (usage/mcp-stats,
    // eval --check|--update-baseline), or a value-taking option whose
    // missing value errors at the encounter point (init --key,
    // quickstart --key/--type — measured: `quickstart --type --version`
    // exits 2 while `quickstart --key bad --version` prints the version).
    // Those parse order-aware and fire version/help at the encounter
    // point, like argparse. watchkeeper joins the set for its
    // choice-validated option VALUES (--format/--fail-on) and value-taking
    // options (--base/--head), measured: `watchkeeper --format bogus
    // --version` exits 2 while `watchkeeper --version --format bogus`
    // prints the version.
    let order_aware = matches!(
        first.as_str(),
        "mcp-stats" | "telemetry" | "usage" | "skill" | "hook" | "eval" | "init" | "quickstart"
            | "migrate" | "watchkeeper" | "corpus"
    );
    if !order_aware {
        if rest.iter().any(|a| a.as_str() == "--version") {
            skip_usage_record();
            print_stdout(&version_line());
            return 0;
        }
        if rest.iter().any(|a| a.as_str() == "-h" || a.as_str() == "--help") {
            skip_usage_record();
            print_stdout(&format!("usage: decided {first} ..."));
            return 0;
        }
    }

    match first.as_str() {
        "validate" => run_validate(&rest),
        "diff" => run_diff(&rest),
        "inspect" => run_inspect(&rest),
        "improve" => run_improve(&rest),
        "relationships" => run_relationships(&rest),
        "stats" => run_stats(&rest),
        "schema" => run_schema(&rest),
        "templates" => run_templates(&rest),
        "resolve" => run_resolve(&rest),
        "find" => run_find(&rest),
        "diagnose" => run_diagnose(&rest),
        "retrieve" => run_retrieve(&rest),
        "review" => run_review(&rest),
        "export" => run_export(&rest),
        "index" => run_index(&rest),
        "portfolio" => run_portfolio(&rest),
        "coverage" => run_coverage(&rest),
        "decisions-for" => run_decisions_for(&rest),
        "gate" => run_gate(&rest),
        "sentry" => run_sentry(&rest),
        "herald" => run_herald(&rest),
        "doctor" => run_doctor(&rest),
        "watchkeeper" => run_watchkeeper(&rest),
        "mcp-stats" => run_mcp_stats(&rest),
        "usage" => run_usage(&rest),
        "telemetry" => run_telemetry(&rest),
        "skill" => run_skill(&rest),
        "hook" => run_hook(&rest),
        "eval" => run_eval(&rest),
        "new" => run_new(&rest),
        "init" => run_init(&rest),
        "quickstart" => run_quickstart(&rest),
        "rename" => run_rename(&rest),
        "migrate" => run_migrate(&rest),
        "corpus" => run_corpus(&rest),
        other => {
            eprintln!("decided-rs: subcommand '{other}' is not yet implemented");
            2
        }
    }
}

struct FlagError(u8);

/// Track the last-seen member of an argparse mutually-exclusive group and
/// error like argparse does on a conflict. Returns the exit code on conflict.
fn mutex_check(prog: &str, new_flag: &str, other_flag: &str, other_set: bool) -> Option<u8> {
    if other_set {
        Some(argparse_error(
            prog,
            &format!("argument {new_flag}: not allowed with argument {other_flag}"),
        ))
    } else {
        None
    }
}

/// Consume a single-argument option's value: the inline `--flag=VALUE` form,
/// or the next token when it reads as a value (bare `-` counts; other
/// `-`-leading tokens do not). A missing value errors immediately at this
/// argv position with `argument <flag>: expected one argument`.
fn take_opt_value(
    prog: &str,
    flag: &str,
    arg: &str,
    rest: &[&String],
    i: &mut usize,
) -> Result<String, u8> {
    if let Some(inline) = arg.strip_prefix(flag).and_then(|r| r.strip_prefix('=')) {
        return Ok(inline.to_string());
    }
    *i += 1;
    match rest.get(*i) {
        Some(v) if !v.starts_with('-') || v.as_str() == "-" => Ok(v.to_string()),
        _ => Err(argparse_error(
            prog,
            &format!("argument {flag}: expected one argument"),
        )),
    }
}

fn run_corpus(rest: &[&String]) -> u8 {
    match rest.first().map(|action| action.as_str()) {
        Some("status") => run_corpus_status(&rest[1..]),
        Some("explain") => run_corpus_explain(&rest[1..]),
        Some("-h" | "--help") => {
            skip_usage_record();
            print_stdout("usage: decided corpus {digest,status,explain} ...");
            0
        }
        _ => run_corpus_digest(rest),
    }
}

fn run_corpus_digest(rest: &[&String]) -> u8 {
    let prog = "decided corpus";
    let mut action: Option<String> = None;
    let mut root: Option<String> = None;
    let mut corpus: Option<String> = None;
    let mut version: u32 = 1;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if action.is_none() {
                if arg != "digest" {
                    return argparse_error(
                        prog,
                        &format!(
                            "argument action: invalid choice: '{arg}' (choose from 'digest', 'status', 'explain')"
                        ),
                    );
                }
                action = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            i += 1;
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "-h" | "--help" => {
                skip_usage_record();
                print_stdout(
                    "usage: decided corpus digest --root ROOT --corpus CORPUS [--version 2]",
                );
                return 0;
            }
            other if other == "--root" || other.starts_with("--root=") => {
                match take_opt_value(prog, "--root", other, rest, &mut i) {
                    Ok(value) => root = Some(value),
                    Err(code) => return code,
                }
            }
            other if other == "--corpus" || other.starts_with("--corpus=") => {
                match take_opt_value(prog, "--corpus", other, rest, &mut i) {
                    Ok(value) => corpus = Some(value),
                    Err(code) => return code,
                }
            }
            other if other == "--version" || other.starts_with("--version=") => {
                match take_opt_value(prog, "--version", other, rest, &mut i) {
                    Ok(value) if value == "2" => version = 2,
                    Ok(value) => {
                        return argparse_error(
                            prog,
                            &format!("argument --version: invalid choice: '{value}' (choose from '2')"),
                        )
                    }
                    Err(code) => return code,
                }
            }
            other => extras.push(other.to_string()),
        }
        i += 1;
    }

    if action.is_none() {
        return argparse_error(prog, "the following arguments are required: action");
    }
    if root.is_none() || corpus.is_none() {
        let missing = match (root.is_none(), corpus.is_none()) {
            (true, true) => "--root, --corpus",
            (true, false) => "--root",
            (false, true) => "--corpus",
            (false, false) => unreachable!(),
        };
        return argparse_error(
            prog,
            &format!("the following arguments are required: {missing}"),
        );
    }
    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_corpus_digest(&CorpusDigestArgs {
        root: root.expect("checked above"),
        corpus: corpus.expect("checked above"),
        version,
    }) as u8
}

fn run_corpus_status(rest: &[&String]) -> u8 {
    let mut directory: Option<String> = None;
    let mut json = false;
    let mut extras = Vec::new();
    let mut positional_only = false;

    for arg in rest {
        let arg = arg.as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--json" => json = true,
            "--version" => {
                skip_usage_record();
                print_stdout(&version_line());
                return 0;
            }
            "-h" | "--help" => {
                skip_usage_record();
                print_stdout("usage: decided corpus status [DIRECTORY] [--json]");
                return 0;
            }
            other => extras.push(other.to_string()),
        }
    }
    if !extras.is_empty() {
        return unrecognized(&extras);
    }
    cmd_corpus_status(&CorpusStatusArgs {
        directory: directory.unwrap_or_else(default_corpus_directory),
        json,
    }) as u8
}

fn run_corpus_explain(rest: &[&String]) -> u8 {
    let prog = "decided corpus explain";
    let mut reference: Option<String> = None;
    let mut directory: Option<String> = None;
    let mut context: Option<String> = None;
    let mut json = false;
    let mut extras = Vec::new();
    let mut positional_only = false;
    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if reference.is_none() {
                reference = Some(arg.to_string());
            } else if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            i += 1;
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--json" => json = true,
            "--version" => {
                skip_usage_record();
                print_stdout(&version_line());
                return 0;
            }
            "-h" | "--help" => {
                skip_usage_record();
                print_stdout(
                    "usage: decided corpus explain REFERENCE [DIRECTORY] [--from SOURCE] [--json]",
                );
                return 0;
            }
            other if other == "--from" || other.starts_with("--from=") => {
                match take_opt_value(prog, "--from", other, rest, &mut i) {
                    Ok(value) => context = Some(value),
                    Err(code) => return code,
                }
            }
            other => extras.push(other.to_string()),
        }
        i += 1;
    }
    let Some(reference) = reference else {
        return argparse_error(prog, "the following arguments are required: reference");
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }
    cmd_corpus_explain(&CorpusExplainArgs {
        reference,
        directory: directory.unwrap_or_else(default_corpus_directory),
        context,
        json,
    }) as u8
}

fn default_corpus_directory() -> String {
    if std::path::Path::new("decisions").is_dir() {
        "decisions".to_string()
    } else {
        ".".to_string()
    }
}

fn run_validate(rest: &[&String]) -> u8 {
    let prog = "decided validate";
    let mut file: Option<String> = None;
    let mut json = false;
    let mut sarif = false;
    let mut top_level = false;
    let mut corpus: Option<String> = None;
    let mut cache = true;
    let mut verify = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if file.is_none() {
                file = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            i += 1;
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--json" => {
                if let Some(code) = mutex_check(prog, "--json", "--sarif", sarif) {
                    return code;
                }
                json = true;
            }
            "--sarif" => {
                if let Some(code) = mutex_check(prog, "--sarif", "--json", json) {
                    return code;
                }
                sarif = true;
            }
            "--top-level" => top_level = true,
            "--recursive" => {} // affirmation of the default
            "--cache" => cache = true,
            "--no-cache" => cache = false,
            "--verify" => verify = true,
            other if other == "--corpus" || other.starts_with("--corpus=") => {
                match take_opt_value(prog, "--corpus", other, rest, &mut i) {
                    Ok(v) => corpus = Some(v),
                    Err(code) => return code,
                }
            }
            other => extras.push(other.to_string()),
        }
        i += 1;
    }

    let Some(file) = file else {
        return argparse_error(prog, "the following arguments are required: file");
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_validate(&ValidateArgs {
        file,
        json,
        sarif,
        top_level,
        corpus,
        cache,
        verify,
    }) as u8
}

fn run_diff(rest: &[&String]) -> u8 {
    let prog = "decided diff";
    let mut old: Option<String> = None;
    let mut new: Option<String> = None;
    let mut json = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    for arg in rest {
        let arg = arg.as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if old.is_none() {
                old = Some(arg.to_string());
            } else if new.is_none() {
                new = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--json" => json = true,
            other => extras.push(other.to_string()),
        }
    }

    // argparse reports every still-missing required positional at once:
    // neither given -> "old, new"; only `old` given -> "new".
    let (Some(old), Some(new)) = (old.clone(), new) else {
        let missing = if old.is_none() { "old, new" } else { "new" };
        return argparse_error(
            prog,
            &format!("the following arguments are required: {missing}"),
        );
    };
    if !extras.is_empty() {
        // Leftover positionals surface as the TOP-LEVEL parser's error.
        return unrecognized(&extras);
    }

    cmd_diff(&DiffArgs { old, new, json }) as u8
}

fn run_inspect(rest: &[&String]) -> u8 {
    let prog = "decided inspect";
    let mut file: Option<String> = None;
    let mut verbose = false;
    let mut top_level = false;
    let mut json = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    for arg in rest {
        let arg = arg.as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if file.is_none() {
                file = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--verbose" => verbose = true,
            "--top-level" => top_level = true,
            "--recursive" => {} // affirmation of the default
            "--json" => json = true,
            other => extras.push(other.to_string()),
        }
    }

    let Some(file) = file else {
        return argparse_error(prog, "the following arguments are required: file");
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_inspect(&InspectArgs {
        file,
        verbose,
        top_level,
        json,
    }) as u8
}

fn run_improve(rest: &[&String]) -> u8 {
    let prog = "decided improve";
    let mut file: Option<String> = None;
    let mut json = false;
    let mut template = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    for arg in rest {
        let arg = arg.as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if file.is_none() {
                file = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            continue;
        }
        match arg {
            "--" => positional_only = true,
            // `--json | --template` is a local mutually-exclusive group
            // (improve does NOT inherit json_parent).
            "--json" => {
                if let Some(code) = mutex_check(prog, "--json", "--template", template) {
                    return code;
                }
                json = true;
            }
            "--template" => {
                if let Some(code) = mutex_check(prog, "--template", "--json", json) {
                    return code;
                }
                template = true;
            }
            other => extras.push(other.to_string()),
        }
    }

    let Some(file) = file else {
        return argparse_error(prog, "the following arguments are required: file");
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_improve(&ImproveArgs {
        file,
        json,
        template,
    }) as u8
}

fn run_relationships(rest: &[&String]) -> u8 {
    let prog = "decided relationships";
    let mut path: Option<String> = None;
    let mut validate = false;
    let mut sarif = false;
    let mut json = false;
    let mut top_level = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    for arg in rest {
        let arg = arg.as_str();
        if positional_only || !arg.starts_with('-') {
            if path.is_none() {
                path = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--validate" => validate = true,
            "--sarif" => sarif = true,
            "--json" => json = true,
            "--top-level" => top_level = true,
            "--recursive" => {}
            other => extras.push(other.to_string()),
        }
    }

    let Some(path) = path else {
        return argparse_error(prog, "the following arguments are required: path");
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_relationships(&RelationshipsArgs {
        path,
        validate,
        sarif,
        json,
        top_level,
    }) as u8
}

fn run_stats(rest: &[&String]) -> u8 {
    let prog = "decided stats";
    let mut directory: Option<String> = None;
    let mut json = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    for arg in rest {
        let arg = arg.as_str();
        if positional_only || !arg.starts_with('-') {
            if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--json" => json = true,
            other => extras.push(other.to_string()),
        }
    }

    let Some(directory) = directory else {
        return argparse_error(prog, "the following arguments are required: directory");
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_stats(&StatsArgs { directory, json }) as u8
}

fn run_portfolio(rest: &[&String]) -> u8 {
    let prog = "decided portfolio";
    let mut directory: Option<String> = None;
    let mut json = false;
    let mut top_level = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    for arg in rest {
        let arg = arg.as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--json" => json = true,
            "--top-level" => top_level = true,
            "--recursive" => {} // affirmation of the default
            other => extras.push(other.to_string()),
        }
    }

    // `directory` is REQUIRED here, unlike the sibling index/export parsers.
    let Some(directory) = directory else {
        return argparse_error(prog, "the following arguments are required: directory");
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_portfolio(&PortfolioArgs {
        directory,
        json,
        top_level,
    }) as u8
}

/// `decided index [directory] [--json] [--top-level]` — optional positional
/// (default '.', like the sibling export parser), version/json/scope
/// parents. No cache flags: index never consumes the cache.
fn run_index(rest: &[&String]) -> u8 {
    let mut directory: Option<String> = None;
    let mut json = false;
    let mut top_level = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    for arg in rest {
        let arg = arg.as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--json" => json = true,
            "--top-level" => top_level = true,
            "--recursive" => {} // affirmation of the default
            other => extras.push(other.to_string()),
        }
    }

    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_index(&IndexArgs {
        directory: directory.unwrap_or_else(|| ".".to_string()),
        json,
        top_level,
    }) as u8
}

fn run_coverage(rest: &[&String]) -> u8 {
    // Optional positional (default '.'); json_parent only — an unknown flag
    // (e.g. --top-level) bubbles to the TOP-LEVEL parser's error.
    let mut directory: Option<String> = None;
    let mut json = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    for arg in rest {
        let arg = arg.as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--json" => json = true,
            other => extras.push(other.to_string()),
        }
    }

    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_coverage(&CoverageArgs {
        directory: directory.unwrap_or_else(|| ".".to_string()),
        json,
    }) as u8
}

fn run_decisions_for(rest: &[&String]) -> u8 {
    let prog = "decided decisions-for";
    let mut path: Option<String> = None;
    let mut directory: Option<String> = None;
    let mut json = false;
    let mut top_level = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    for arg in rest {
        let arg = arg.as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if path.is_none() {
                path = Some(arg.to_string());
            } else if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--json" => json = true,
            "--top-level" => top_level = true,
            "--recursive" => {} // affirmation of the default
            other => extras.push(other.to_string()),
        }
    }

    let Some(path) = path else {
        return argparse_error(prog, "the following arguments are required: path");
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_decisions_for(&DecisionsForArgs {
        path,
        directory: directory.unwrap_or_else(|| ".".to_string()),
        json,
        top_level,
    }) as u8
}

fn run_gate(rest: &[&String]) -> u8 {
    let prog = "decided gate";
    let mut directory: Option<String> = None;
    let mut json = false;
    let mut sarif = false;
    let mut top_level = false;
    let mut code = false;
    let mut repository = ".".to_string();
    let mut base: Option<String> = None;
    let mut full = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            i += 1;
            continue;
        }
        match arg {
            "--" => positional_only = true,
            // `--json | --sarif` is a mutually-exclusive group (like validate).
            "--json" => {
                if let Some(code) = mutex_check(prog, "--json", "--sarif", sarif) {
                    return code;
                }
                json = true;
            }
            "--sarif" => {
                if let Some(code) = mutex_check(prog, "--sarif", "--json", json) {
                    return code;
                }
                sarif = true;
            }
            "--top-level" => top_level = true,
            "--code" => code = true,
            "--full" => full = true,
            "--repository" | "--base" => {
                i += 1;
                if i >= rest.len() {
                    return argparse_error(prog, &format!("argument {arg}: expected one argument"));
                }
                if arg == "--repository" {
                    repository = rest[i].to_string();
                } else {
                    base = Some(rest[i].to_string());
                }
            }
            // NO --recursive here (gate declares --top-level inline, not
            // scope_parent) — it bubbles to the top-level parser's error.
            other => extras.push(other.to_string()),
        }
        i += 1;
    }

    // `directory` is a REQUIRED positional — unlike doctor/coverage.
    let Some(directory) = directory else {
        return argparse_error(prog, "the following arguments are required: directory");
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_gate(&GateArgs {
        directory,
        json,
        sarif,
        top_level,
        code,
        repository,
        base,
        full,
    }) as u8
}

fn run_sentry(rest: &[&String]) -> u8 {
    let prog = "decided sentry";
    let mut directory: Option<String> = None;
    let mut repository = ".".to_string();
    let mut base: Option<String> = None;
    let mut full = false;
    let mut json = false;
    let mut sarif = false;
    let mut top_level = false;
    let mut extras = Vec::new();
    let mut positional_only = false;
    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            i += 1;
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--json" => {
                if let Some(code) = mutex_check(prog, "--json", "--sarif", sarif) {
                    return code;
                }
                json = true;
            }
            "--sarif" => {
                if let Some(code) = mutex_check(prog, "--sarif", "--json", json) {
                    return code;
                }
                sarif = true;
            }
            "--full" => full = true,
            "--top-level" => top_level = true,
            "--repository" | "--base" => {
                i += 1;
                if i >= rest.len() {
                    return argparse_error(prog, &format!("argument {arg}: expected one argument"));
                }
                if arg == "--repository" {
                    repository = rest[i].to_string();
                } else {
                    base = Some(rest[i].to_string());
                }
            }
            other => extras.push(other.to_string()),
        }
        i += 1;
    }
    let Some(directory) = directory else {
        return argparse_error(prog, "the following arguments are required: directory");
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }
    cmd_sentry(&SentryArgs {
        directory,
        repository,
        base,
        full,
        json,
        sarif,
        top_level,
    }) as u8
}

fn run_herald(rest: &[&String]) -> u8 {
    let prog = "decided herald";
    let mut directory = None;
    let mut paths_file = None;
    let mut link_base = String::new();
    let mut max_inline = 5i64;
    let mut out = None;
    let mut github_output = None;
    let mut top_level = false;
    let mut extras = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if arg == "-" || !arg.starts_with('-') {
            if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            i += 1;
            continue;
        }
        match arg {
            "--top-level" => top_level = true,
            "--paths-file" | "--link-base" | "--max-inline" | "--out" | "--github-output" => {
                i += 1;
                if i >= rest.len() {
                    return argparse_error(prog, &format!("argument {arg}: expected one argument"));
                }
                let value = rest[i].to_string();
                match arg {
                    "--paths-file" => paths_file = Some(value),
                    "--link-base" => link_base = value,
                    "--max-inline" => {
                        max_inline = match value.parse() {
                            Ok(value) => value,
                            Err(_) => {
                                return argparse_error(
                                    prog,
                                    &format!("argument --max-inline: invalid int value: '{value}'"),
                                )
                            }
                        }
                    }
                    "--out" => out = Some(value),
                    "--github-output" => github_output = Some(value),
                    _ => unreachable!(),
                }
            }
            other => extras.push(other.to_string()),
        }
        i += 1;
    }
    let Some(directory) = directory else {
        return argparse_error(prog, "the following arguments are required: directory");
    };
    let Some(paths_file) = paths_file else {
        return argparse_error(prog, "the following arguments are required: --paths-file");
    };
    let Some(out) = out else {
        return argparse_error(prog, "the following arguments are required: --out");
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }
    cmd_herald(&HeraldArgs {
        directory,
        paths_file,
        link_base,
        max_inline,
        out,
        github_output,
        top_level,
    }) as u8
}

fn run_doctor(rest: &[&String]) -> u8 {
    let prog = "decided doctor";
    let mut directory: Option<String> = None;
    let mut json = false;
    let mut top_level = false;
    let mut hub_threshold: i64 = 20; // doctor.DEFAULT_HUB_THRESHOLD
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') || looks_like_negative_number(arg)
        {
            if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            i += 1;
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--json" => json = true,
            "--top-level" => top_level = true,
            "--recursive" => {} // affirmation of the default (scope_parent)
            "--hub-threshold" => {
                // type=int: the next token is a value when it does not look
                // like an option (bare `-` and negative numbers count).
                i += 1;
                let raw = match rest.get(i) {
                    Some(v)
                        if !v.starts_with('-')
                            || v.as_str() == "-"
                            || looks_like_negative_number(v) =>
                    {
                        v.as_str()
                    }
                    _ => {
                        return argparse_error(
                            prog,
                            "argument --hub-threshold: expected one argument",
                        )
                    }
                };
                match py_parse_int(raw) {
                    Some(v) => hub_threshold = v,
                    None => {
                        return argparse_error(
                            prog,
                            &format!("argument --hub-threshold: invalid int value: '{raw}'"),
                        )
                    }
                }
            }
            other if other.starts_with("--hub-threshold=") => {
                let raw = &other["--hub-threshold=".len()..];
                match py_parse_int(raw) {
                    Some(v) => hub_threshold = v,
                    None => {
                        return argparse_error(
                            prog,
                            &format!("argument --hub-threshold: invalid int value: '{raw}'"),
                        )
                    }
                }
            }
            other => extras.push(other.to_string()),
        }
        i += 1;
    }

    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_doctor(&DoctorArgs {
        directory: directory.unwrap_or_else(|| ".".to_string()),
        json,
        top_level,
        hub_threshold,
    }) as u8
}

/// The shared `[--json | --share]` parser of `mcp-stats` and `usage`
/// (identical argparse shapes, no positional). Order-aware: the mutex
/// error fires at the ENCOUNTER of the conflicting flag (so it beats a
/// later `--version`), unknown tokens defer to the root parser's
/// `unrecognized arguments` at end-of-parse (so an earlier `--version`
/// beats them), exactly like argparse.
fn parse_json_share_group(prog: &str, rest: &[&String]) -> Result<(bool, bool), u8> {
    let mut json = false;
    let mut share = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    for arg in rest {
        let arg = arg.as_str();
        if positional_only {
            extras.push(arg.to_string());
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--version" => {
                skip_usage_record();
                print_stdout(&version_line());
                return Err(0);
            }
            "-h" | "--help" => {
                skip_usage_record();
                print_stdout(&format!("usage: {prog} ..."));
                return Err(0);
            }
            "--json" => {
                if let Some(code) = mutex_check(prog, "--json", "--share", share) {
                    return Err(code);
                }
                json = true;
            }
            "--share" => {
                if let Some(code) = mutex_check(prog, "--share", "--json", json) {
                    return Err(code);
                }
                share = true;
            }
            other => extras.push(other.to_string()),
        }
    }

    if !extras.is_empty() {
        return Err(unrecognized(&extras));
    }
    Ok((json, share))
}

fn run_mcp_stats(rest: &[&String]) -> u8 {
    match parse_json_share_group("decided mcp-stats", rest) {
        Ok((json, share)) => cmd_mcp_stats(&McpStatsArgs { json, share }) as u8,
        Err(code) => code,
    }
}

fn run_usage(rest: &[&String]) -> u8 {
    match parse_json_share_group("decided usage", rest) {
        Ok((json, share)) => cmd_usage(&UsageArgs { json, share }) as u8,
        Err(code) => code,
    }
}

fn run_telemetry(rest: &[&String]) -> u8 {
    let prog = "decided telemetry";
    let mut action: Option<String> = None;
    let mut enterprise = false;
    let mut unlock = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    for arg in rest {
        let arg = arg.as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if action.is_none() {
                // The positional's choice set is validated when the token
                // is CONSUMED, so an invalid choice beats a later
                // `--version` (measured: `telemetry bogus --version` exits
                // 2, `telemetry --version bogus` prints the version).
                if !matches!(arg, "on" | "off" | "status") {
                    return argparse_error(
                        prog,
                        &format!(
                            "argument action: invalid choice: '{arg}' (choose from 'on', 'off', 'status')"
                        ),
                    );
                }
                action = Some(arg.to_string());
            } else {
                // A second positional is an end-of-parse `unrecognized
                // arguments` — deferred, so a later `--version` wins.
                extras.push(arg.to_string());
            }
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--version" => {
                skip_usage_record();
                print_stdout(&version_line());
                return 0;
            }
            "-h" | "--help" => {
                skip_usage_record();
                print_stdout(&format!("usage: {prog} ..."));
                return 0;
            }
            "--enterprise" => enterprise = true,
            "--unlock" => unlock = true,
            other => extras.push(other.to_string()),
        }
    }

    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_telemetry(&TelemetryArgs {
        action: action.unwrap_or_else(|| "status".to_string()),
        enterprise,
        unlock,
    }) as u8
}

/// `decided skill <action> [name] [--dir DIR] [--json]` — order-aware: the
/// `action` positional's choice set is validated when the token is
/// CONSUMED (an invalid action beats a later `--version`; an earlier
/// `--version` wins), a second positional defers to the end-of-parse
/// `unrecognized arguments`, exactly like argparse.
fn run_skill(rest: &[&String]) -> u8 {
    let prog = "decided skill";
    let mut action: Option<String> = None;
    let mut name: Option<String> = None;
    let mut dir: String = ".".to_string();
    let mut json = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if action.is_none() {
                if !matches!(arg, "install" | "list") {
                    return argparse_error(
                        prog,
                        &format!(
                            "argument action: invalid choice: '{arg}' (choose from 'install', 'list')"
                        ),
                    );
                }
                action = Some(arg.to_string());
            } else if name.is_none() {
                name = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            i += 1;
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--version" => {
                skip_usage_record();
                print_stdout(&version_line());
                return 0;
            }
            "-h" | "--help" => {
                skip_usage_record();
                print_stdout(&format!("usage: {prog} ..."));
                return 0;
            }
            "--json" => json = true,
            other if other == "--dir" || other.starts_with("--dir=") => {
                match take_opt_value(prog, "--dir", other, rest, &mut i) {
                    Ok(v) => dir = v,
                    Err(code) => return code,
                }
            }
            other => extras.push(other.to_string()),
        }
        i += 1;
    }

    let Some(action) = action else {
        return argparse_error(prog, "the following arguments are required: action");
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_skill(&SkillArgs {
        action,
        name,
        dir,
        json,
    }) as u8
}

/// `decided hook <action> [--style STYLE] [--dir DIR] [--json]` — order-aware
/// like skill; additionally `--style`'s choice set is validated when its
/// VALUE is consumed (so `--style bogus --version` exits 2 while
/// `--version --style bogus` prints the version), making the service-level
/// unknown-style error unreachable via the CLI.
fn run_hook(rest: &[&String]) -> u8 {
    let prog = "decided hook";
    let mut action: Option<String> = None;
    let mut style: String = "post-commit".to_string(); // hooks.DEFAULT_STYLE
    let mut dir: String = ".".to_string();
    let mut json = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    let style_choice = |prog: &str, v: &str| -> Option<u8> {
        if matches!(v, "post-commit" | "pre-commit") {
            None
        } else {
            Some(argparse_error(
                prog,
                &format!(
                    "argument --style: invalid choice: '{v}' (choose from 'post-commit', 'pre-commit')"
                ),
            ))
        }
    };

    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if action.is_none() {
                if !matches!(arg, "install" | "list") {
                    return argparse_error(
                        prog,
                        &format!(
                            "argument action: invalid choice: '{arg}' (choose from 'install', 'list')"
                        ),
                    );
                }
                action = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            i += 1;
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--version" => {
                skip_usage_record();
                print_stdout(&version_line());
                return 0;
            }
            "-h" | "--help" => {
                skip_usage_record();
                print_stdout(&format!("usage: {prog} ..."));
                return 0;
            }
            "--json" => json = true,
            other if other == "--style" || other.starts_with("--style=") => {
                match take_opt_value(prog, "--style", other, rest, &mut i) {
                    Ok(v) => {
                        if let Some(code) = style_choice(prog, &v) {
                            return code;
                        }
                        style = v;
                    }
                    Err(code) => return code,
                }
            }
            other if other == "--dir" || other.starts_with("--dir=") => {
                match take_opt_value(prog, "--dir", other, rest, &mut i) {
                    Ok(v) => dir = v,
                    Err(code) => return code,
                }
            }
            other => extras.push(other.to_string()),
        }
        i += 1;
    }

    let Some(action) = action else {
        return argparse_error(prog, "the following arguments are required: action");
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_hook(&HookArgs {
        action,
        style,
        dir,
        json,
    }) as u8
}

/// `decided watchkeeper [directory] [--base REV] [--head REV]
/// [--format {human,json,github}] [--json] [--fail-on {error,warning,none}]
/// [--no-annotate]` — order-aware: `--format`/`--fail-on` are
/// argparse-choice-validated when their VALUE is consumed and a missing
/// `--base`/`--head` value errors at its own position (each beats a later
/// `--version`; an earlier `--version` wins). The directory positional is a
/// free string (a bad directory defers to dispatch, so `--version` after it
/// still wins), and extra positionals defer to the end-of-parse
/// `unrecognized arguments`.
fn run_watchkeeper(rest: &[&String]) -> u8 {
    let prog = "decided watchkeeper";
    let mut directory: Option<String> = None;
    let mut base: String = "main".to_string();
    let mut head: Option<String> = None;
    let mut format: String = "human".to_string();
    let mut json = false;
    let mut fail_on: String = "error".to_string();
    let mut annotate = true;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    let format_choice = |v: &str| -> Option<u8> {
        if matches!(v, "human" | "json" | "github") {
            None
        } else {
            Some(argparse_error(
                prog,
                &format!(
                    "argument --format: invalid choice: '{v}' (choose from 'human', 'json', 'github')"
                ),
            ))
        }
    };
    let fail_on_choice = |v: &str| -> Option<u8> {
        if matches!(v, "error" | "warning" | "none") {
            None
        } else {
            Some(argparse_error(
                prog,
                &format!(
                    "argument --fail-on: invalid choice: '{v}' (choose from 'error', 'warning', 'none')"
                ),
            ))
        }
    };

    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            i += 1;
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--version" => {
                skip_usage_record();
                print_stdout(&version_line());
                return 0;
            }
            "-h" | "--help" => {
                skip_usage_record();
                print_stdout(&format!("usage: {prog} ..."));
                return 0;
            }
            "--json" => json = true,
            "--no-annotate" => annotate = false,
            other if other == "--base" || other.starts_with("--base=") => {
                match take_opt_value(prog, "--base", other, rest, &mut i) {
                    Ok(v) => base = v,
                    Err(code) => return code,
                }
            }
            other if other == "--head" || other.starts_with("--head=") => {
                match take_opt_value(prog, "--head", other, rest, &mut i) {
                    Ok(v) => head = Some(v),
                    Err(code) => return code,
                }
            }
            other if other == "--format" || other.starts_with("--format=") => {
                match take_opt_value(prog, "--format", other, rest, &mut i) {
                    Ok(v) => {
                        if let Some(code) = format_choice(&v) {
                            return code;
                        }
                        format = v;
                    }
                    Err(code) => return code,
                }
            }
            other if other == "--fail-on" || other.starts_with("--fail-on=") => {
                match take_opt_value(prog, "--fail-on", other, rest, &mut i) {
                    Ok(v) => {
                        if let Some(code) = fail_on_choice(&v) {
                            return code;
                        }
                        fail_on = v;
                    }
                    Err(code) => return code,
                }
            }
            other => extras.push(other.to_string()),
        }
        i += 1;
    }

    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_watchkeeper(&WatchkeeperArgs {
        directory,
        base,
        head,
        format,
        json,
        fail_on,
        annotate,
    }) as u8
}

/// `decided eval [--check | --update-baseline] [--json] [--root ROOT]
/// [--queries QUERIES] [--baseline BASELINE] [--config CONFIG]` — no
/// positionals; the mode mutex errors at the ENCOUNTER of the conflicting
/// flag (so it beats a later `--version`), like argparse.
fn run_eval(rest: &[&String]) -> u8 {
    let prog = "decided eval";
    let mut check = false;
    let mut update_baseline = false;
    let mut json = false;
    let mut root: String = "rust/fixtures/eval/corpus".to_string();
    let mut queries: String = "rust/fixtures/eval/queries.json".to_string();
    let mut baseline: String = "rust/fixtures/eval/baseline.json".to_string();
    let mut config: String = "rust/fixtures/eval/eval-config.json".to_string();
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            extras.push(arg.to_string());
            i += 1;
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--version" => {
                skip_usage_record();
                print_stdout(&version_line());
                return 0;
            }
            "-h" | "--help" => {
                skip_usage_record();
                print_stdout(&format!("usage: {prog} ..."));
                return 0;
            }
            "--check" => {
                if let Some(code) =
                    mutex_check(prog, "--check", "--update-baseline", update_baseline)
                {
                    return code;
                }
                check = true;
            }
            "--update-baseline" => {
                if let Some(code) = mutex_check(prog, "--update-baseline", "--check", check) {
                    return code;
                }
                update_baseline = true;
            }
            "--json" => json = true,
            other if other == "--root" || other.starts_with("--root=") => {
                match take_opt_value(prog, "--root", other, rest, &mut i) {
                    Ok(v) => root = v,
                    Err(code) => return code,
                }
            }
            other if other == "--queries" || other.starts_with("--queries=") => {
                match take_opt_value(prog, "--queries", other, rest, &mut i) {
                    Ok(v) => queries = v,
                    Err(code) => return code,
                }
            }
            other if other == "--baseline" || other.starts_with("--baseline=") => {
                match take_opt_value(prog, "--baseline", other, rest, &mut i) {
                    Ok(v) => baseline = v,
                    Err(code) => return code,
                }
            }
            other if other == "--config" || other.starts_with("--config=") => {
                match take_opt_value(prog, "--config", other, rest, &mut i) {
                    Ok(v) => config = v,
                    Err(code) => return code,
                }
            }
            other => extras.push(other.to_string()),
        }
        i += 1;
    }

    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_eval(&EvalArgs {
        check,
        update_baseline,
        json,
        root,
        queries,
        baseline,
        config,
    }) as u8
}

fn run_resolve(rest: &[&String]) -> u8 {
    let prog = "decided resolve";
    let mut id: Option<String> = None;
    let mut directory: Option<String> = None;
    let mut json = false;
    let mut top_level = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    for arg in rest {
        let arg = arg.as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if id.is_none() {
                id = Some(arg.to_string());
            } else if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--json" => json = true,
            "--top-level" => top_level = true,
            "--recursive" => {} // affirmation of the default
            other => extras.push(other.to_string()),
        }
    }

    let Some(id) = id else {
        return argparse_error(prog, "the following arguments are required: id");
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_resolve(&ResolveArgs {
        id,
        directory: directory.unwrap_or_else(|| ".".to_string()),
        json,
        top_level,
    }) as u8
}

fn run_find(rest: &[&String]) -> u8 {
    let prog = "decided find";
    let mut query: Option<String> = None;
    let mut directory: Option<String> = None;
    let mut artifact_type: Option<String> = None;
    let mut decisions = false;
    let mut tags: Vec<String> = Vec::new();
    let mut json = false;
    let mut explain = false;
    let mut top_level = false;
    let mut live = false;
    let mut cache = true;
    let mut verify = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if query.is_none() {
                query = Some(arg.to_string());
            } else if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            i += 1;
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--json" => json = true,
            "--explain" => explain = true,
            "--top-level" => top_level = true,
            "--live" => live = true, // the live-only facet (ADR-113)
            "--recursive" => {} // affirmation of the default
            "--cache" => cache = true,
            "--no-cache" => cache = false,
            "--verify" => verify = true,
            "--decisions" => {
                // Mutually exclusive with --type (argparse group).
                if let Some(code) =
                    mutex_check(prog, "--decisions", "--type", artifact_type.is_some())
                {
                    return code;
                }
                decisions = true;
            }
            other if other == "--type" || other.starts_with("--type=") => {
                if let Some(code) = mutex_check(prog, "--type", "--decisions", decisions) {
                    return code;
                }
                match take_opt_value(prog, "--type", other, rest, &mut i) {
                    Ok(v) => artifact_type = Some(v),
                    Err(code) => return code,
                }
            }
            other if other == "--tag" || other.starts_with("--tag=") => {
                match take_opt_value(prog, "--tag", other, rest, &mut i) {
                    Ok(v) => tags.push(v),
                    Err(code) => return code,
                }
            }
            other => extras.push(other.to_string()),
        }
        i += 1;
    }

    let Some(query) = query else {
        return argparse_error(prog, "the following arguments are required: query");
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_find(&FindArgs {
        query,
        directory: directory.unwrap_or_else(|| ".".to_string()),
        artifact_type,
        decisions,
        tags,
        json,
        explain,
        top_level,
        live,
        cache,
        verify,
    }) as u8
}

fn run_diagnose(rest: &[&String]) -> u8 {
    let prog = "decided diagnose";
    let mut query: Option<String> = None;
    let mut target: Option<String> = None;
    let mut directory: Option<String> = None;
    let mut artifact_type: Option<String> = None;
    let mut tags: Vec<String> = Vec::new();
    let mut surface_limit: usize = 5;
    let mut json = false;
    let mut top_level = false;
    let mut live = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if query.is_none() {
                query = Some(arg.to_string());
            } else if target.is_none() {
                target = Some(arg.to_string());
            } else if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            i += 1;
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--json" => json = true,
            "--top-level" => top_level = true,
            "--live" => live = true,
            "--recursive" => {}
            other if other == "--type" || other.starts_with("--type=") => {
                match take_opt_value(prog, "--type", other, rest, &mut i) {
                    Ok(value) => artifact_type = Some(value),
                    Err(code) => return code,
                }
            }
            other if other == "--tag" || other.starts_with("--tag=") => {
                match take_opt_value(prog, "--tag", other, rest, &mut i) {
                    Ok(value) => tags.push(value),
                    Err(code) => return code,
                }
            }
            other if other == "--limit" || other.starts_with("--limit=") => {
                let value = match take_opt_value(prog, "--limit", other, rest, &mut i) {
                    Ok(value) => value,
                    Err(code) => return code,
                };
                let Some(parsed) = py_parse_int(&value) else {
                    return argparse_error(
                        prog,
                        &format!("argument --limit: invalid int value: {value:?}"),
                    );
                };
                if parsed < 1 {
                    return argparse_error(prog, "argument --limit: must be at least 1");
                }
                surface_limit = parsed as usize;
            }
            other => extras.push(other.to_string()),
        }
        i += 1;
    }

    let Some(query) = query else {
        return argparse_error(prog, "the following arguments are required: query, target");
    };
    let Some(target) = target else {
        return argparse_error(prog, "the following arguments are required: target");
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_diagnose(&DiagnoseArgs {
        query,
        target,
        directory: directory.unwrap_or_else(|| ".".to_string()),
        artifact_type,
        tags,
        surface_limit,
        json,
        top_level,
        live,
    }) as u8
}

/// `int(value)` for argparse `type=int`: Python-style strip, optional sign,
/// ASCII digits with single interior underscores. (Non-ASCII digit forms are
/// out of scope for the parity surface.)
fn py_parse_int(value: &str) -> Option<i64> {
    let text = crate::pycompat::py_strip(value);
    let (neg, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    if digits.is_empty() {
        return None;
    }
    let bytes = digits.as_bytes();
    if bytes[0] == b'_' || bytes[bytes.len() - 1] == b'_' {
        return None;
    }
    let mut out: i64 = 0;
    let mut prev_underscore = false;
    for &b in bytes {
        if b == b'_' {
            if prev_underscore {
                return None;
            }
            prev_underscore = true;
            continue;
        }
        prev_underscore = false;
        if !b.is_ascii_digit() {
            return None;
        }
        out = out
            .saturating_mul(10)
            .saturating_add(i64::from(b - b'0'));
    }
    Some(if neg { -out } else { out })
}

fn run_retrieve(rest: &[&String]) -> u8 {
    let prog = "decided retrieve";
    let mut task: Option<String> = None;
    let mut directory: Option<String> = None;
    let mut scope: Option<String> = None;
    let mut top_k: i64 = 5;
    let mut budget: i64 = 10_000;
    let mut live = false;
    let mut all = false;
    let mut json = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    // One int-valued flag consumer: argparse `type=int` + its error line.
    enum IntErr {
        Missing,
        Invalid(String),
    }
    let parse_int_flag = |raw: Option<&&String>| -> Result<i64, IntErr> {
        match raw {
            Some(v)
                if !v.starts_with('-')
                    || v.as_str() == "-"
                    || looks_like_negative_number(v) =>
            {
                py_parse_int(v).ok_or_else(|| IntErr::Invalid(v.to_string()))
            }
            _ => Err(IntErr::Missing),
        }
    };
    let int_flag_error = |flag: &str, err: IntErr| -> u8 {
        match err {
            IntErr::Missing => {
                argparse_error(prog, &format!("argument {flag}: expected one argument"))
            }
            IntErr::Invalid(v) => argparse_error(
                prog,
                &format!("argument {flag}: invalid int value: '{v}'"),
            ),
        }
    };

    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if positional_only || !arg.starts_with('-') || arg == "-" || looks_like_negative_number(arg)
        {
            if task.is_none() {
                task = Some(arg.to_string());
            } else if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            i += 1;
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--json" => json = true,
            "--live" => {
                if let Some(code) = mutex_check(prog, "--live", "--all", all) {
                    return code;
                }
                live = true;
            }
            "--all" => {
                if let Some(code) = mutex_check(prog, "--all", "--live", live) {
                    return code;
                }
                all = true;
            }
            other if other == "--scope" || other.starts_with("--scope=") => {
                match take_opt_value(prog, "--scope", other, rest, &mut i) {
                    Ok(v) => scope = Some(v),
                    Err(code) => return code,
                }
            }
            "--top-k" => {
                i += 1;
                match parse_int_flag(rest.get(i)) {
                    Ok(v) => top_k = v,
                    Err(e) => return int_flag_error("--top-k", e),
                }
            }
            other if other.starts_with("--top-k=") => {
                let v = &other["--top-k=".len()..];
                match py_parse_int(v) {
                    Some(parsed) => top_k = parsed,
                    None => return int_flag_error("--top-k", IntErr::Invalid(v.to_string())),
                }
            }
            "--budget" => {
                i += 1;
                match parse_int_flag(rest.get(i)) {
                    Ok(v) => budget = v,
                    Err(e) => return int_flag_error("--budget", e),
                }
            }
            other if other.starts_with("--budget=") => {
                let v = &other["--budget=".len()..];
                match py_parse_int(v) {
                    Some(parsed) => budget = parsed,
                    None => return int_flag_error("--budget", IntErr::Invalid(v.to_string())),
                }
            }
            other => extras.push(other.to_string()),
        }
        i += 1;
    }

    let Some(task) = task else {
        return argparse_error(prog, "the following arguments are required: task");
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_retrieve(&RetrieveArgs {
        task,
        directory: directory.unwrap_or_else(|| ".".to_string()),
        scope,
        top_k,
        budget,
        all,
        json,
    }) as u8
}

/// argparse treats a token matching `^-\d+$` / `^-\d*\.\d+$` as a value, not an
/// option (the parser has no option strings that look like negative numbers).
fn looks_like_negative_number(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('-') else {
        return false;
    };
    if rest.is_empty() {
        return false;
    }
    let mut seen_dot = false;
    let mut seen_digit = false;
    for ch in rest.chars() {
        if ch == '.' {
            if seen_dot {
                return false;
            }
            seen_dot = true;
        } else if ch.is_ascii_digit() {
            seen_digit = true;
        } else {
            return false;
        }
    }
    seen_digit
}

fn run_review(rest: &[&String]) -> u8 {
    let prog = "decided review";
    let mut directory: Option<String> = None;
    let mut json = false;
    let mut sarif = false;
    let mut top_level = false;
    let mut stale_after: Option<i64> = None;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if positional_only || !arg.starts_with('-') || arg == "-" {
            if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            i += 1;
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--json" => json = true,
            "--sarif" => sarif = true,
            "--top-level" => top_level = true,
            "--recursive" => {}
            "--stale-after" => {
                // nargs="?" const=14: consume the next token only if it is a
                // value (not another option), including a negative number.
                let consume = match rest.get(i + 1) {
                    Some(v) => !v.starts_with('-') || looks_like_negative_number(v),
                    None => false,
                };
                if consume {
                    i += 1;
                    let raw = rest[i].as_str();
                    match raw.trim().parse::<i64>() {
                        Ok(v) => stale_after = Some(v),
                        Err(_) => {
                            return argparse_error(
                                prog,
                                &format!("argument --stale-after: invalid int value: '{raw}'"),
                            )
                        }
                    }
                } else {
                    stale_after = Some(14);
                }
            }
            other if other.starts_with("--stale-after=") => {
                let raw = &other["--stale-after=".len()..];
                match raw.trim().parse::<i64>() {
                    Ok(v) => stale_after = Some(v),
                    Err(_) => {
                        return argparse_error(
                            prog,
                            &format!("argument --stale-after: invalid int value: '{raw}'"),
                        )
                    }
                }
            }
            other => extras.push(other.to_string()),
        }
        i += 1;
    }

    let Some(directory) = directory else {
        return argparse_error(prog, "the following arguments are required: directory");
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_review(&ReviewArgs {
        directory,
        json,
        sarif,
        top_level,
        stale_after,
    }) as u8
}

fn run_export(rest: &[&String]) -> u8 {
    let prog = "decided export";
    let mut directory: Option<String> = None;
    let mut json = false;
    let mut html = false;
    let mut okf = false;
    let mut schema: Option<String> = None;
    let mut schema_mode = false;
    let mut documents = false;
    let mut graph = false;
    let mut agent_rules = false;
    let mut check = false;
    let mut client: Vec<String> = Vec::new();
    let mut out: Option<String> = None;
    let mut local_only = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    // Track the last write-mode flag seen for argparse mutex diagnostics.
    let mut last_mode: Option<&'static str> = None;
    let set_mode = |flag: &'static str,
                        slot: &mut bool,
                        last_mode: &mut Option<&'static str>|
     -> Result<(), FlagError> {
        if let Some(prev) = *last_mode {
            if prev != flag {
                return Err(FlagError(argparse_error(
                    prog,
                    &format!("argument {flag}: not allowed with argument {prev}"),
                )));
            }
        }
        *slot = true;
        *last_mode = Some(flag);
        Ok(())
    };

    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if positional_only || !arg.starts_with('-') || arg == "-" {
            if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            i += 1;
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--json" => json = true,
            "--html" => {
                if let Err(FlagError(c)) = set_mode("--html", &mut html, &mut last_mode) {
                    return c;
                }
            }
            "--okf" => {
                if let Err(FlagError(c)) = set_mode("--okf", &mut okf, &mut last_mode) {
                    return c;
                }
            }
            "--documents" => {
                if let Err(FlagError(c)) = set_mode("--documents", &mut documents, &mut last_mode) {
                    return c;
                }
            }
            "--graph" => {
                if let Err(FlagError(c)) = set_mode("--graph", &mut graph, &mut last_mode) {
                    return c;
                }
            }
            "--schema" => {
                if let Err(FlagError(c)) =
                    set_mode("--schema", &mut schema_mode, &mut last_mode)
                {
                    return c;
                }
                i += 1;
                match rest.get(i) {
                    Some(v) if is_export_schema_choice(v) => schema = Some(v.to_string()),
                    Some(v) if !v.starts_with('-') => {
                        return argparse_error(
                            prog,
                            &format!(
                                "argument --schema: invalid choice: '{v}' (choose from 'viewer', 'documents', 'graph')"
                            ),
                        )
                    }
                    _ => return argparse_error(prog, "argument --schema: expected one argument"),
                }
            }
            other if other.starts_with("--schema=") => {
                if let Err(FlagError(c)) =
                    set_mode("--schema", &mut schema_mode, &mut last_mode)
                {
                    return c;
                }
                let value = &other["--schema=".len()..];
                if is_export_schema_choice(value) {
                    schema = Some(value.to_string());
                } else {
                    return argparse_error(
                        prog,
                        &format!(
                            "argument --schema: invalid choice: '{value}' (choose from 'viewer', 'documents', 'graph')"
                        ),
                    );
                }
            }
            "--agent-rules" => {
                if let Err(FlagError(c)) =
                    set_mode("--agent-rules", &mut agent_rules, &mut last_mode)
                {
                    return c;
                }
            }
            "--check" => check = true,
            "--local-only" => local_only = true,
            "--client" => {
                i += 1;
                match rest.get(i) {
                    Some(v) if is_client_choice(v) => client.push(v.to_string()),
                    Some(v) if !v.starts_with('-') => {
                        return argparse_error(
                            prog,
                            &format!(
                                "argument --client: invalid choice: '{v}' (choose from 'claude', 'agents', 'cursor', 'copilot')"
                            ),
                        )
                    }
                    _ => return argparse_error(prog, "argument --client: expected one argument"),
                }
            }
            other if other.starts_with("--client=") => {
                let v = &other["--client=".len()..];
                if is_client_choice(v) {
                    client.push(v.to_string());
                } else {
                    return argparse_error(
                        prog,
                        &format!(
                            "argument --client: invalid choice: '{v}' (choose from 'claude', 'agents', 'cursor', 'copilot')"
                        ),
                    );
                }
            }
            other if other == "--out" || other.starts_with("--out=") => {
                match take_opt_value(prog, "--out", other, rest, &mut i) {
                    Ok(v) => out = Some(v),
                    Err(code) => return code,
                }
            }
            other => extras.push(other.to_string()),
        }
        i += 1;
    }

    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_export(&ExportArgs {
        directory: directory.unwrap_or_else(|| ".".to_string()),
        json,
        schema,
        graph,
        documents,
        html,
        okf,
        agent_rules,
        check,
        client,
        out,
        local_only,
    }) as u8
}

fn is_client_choice(v: &str) -> bool {
    matches!(v, "claude" | "agents" | "cursor" | "copilot")
}

fn is_export_schema_choice(v: &str) -> bool {
    crate::export::EXPORT_SCHEMA_NAMES.contains(&v)
}

fn run_schema(rest: &[&String]) -> u8 {
    let prog = "decided schema";
    let mut schema: Option<String> = None;
    let mut list = false;
    let mut json = false;
    let mut template = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    for arg in rest {
        let arg = arg.as_str();
        if positional_only || !arg.starts_with('-') {
            if schema.is_none() {
                schema = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--list" => list = true,
            "--json" => {
                if let Some(code) = mutex_check(prog, "--json", "--template", template) {
                    return code;
                }
                json = true;
            }
            "--template" => {
                if let Some(code) = mutex_check(prog, "--template", "--json", json) {
                    return code;
                }
                template = true;
            }
            other => extras.push(other.to_string()),
        }
    }

    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_schema(&SchemaArgs {
        schema,
        list,
        json,
        template,
    }) as u8
}

fn run_templates(rest: &[&String]) -> u8 {
    let mut json = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    for arg in rest {
        let arg = arg.as_str();
        if positional_only || !arg.starts_with('-') {
            extras.push(arg.to_string());
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--json" => json = true,
            other => extras.push(other.to_string()),
        }
    }

    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_templates(&TemplatesArgs { json }) as u8
}

fn run_new(rest: &[&String]) -> u8 {
    let prog = "decided new";
    let mut artifact_type: Option<String> = None;
    let mut output_path: Option<String> = None;
    let mut json = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    for arg in rest {
        let arg = arg.as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if artifact_type.is_none() {
                artifact_type = Some(arg.to_string());
            } else if output_path.is_none() {
                output_path = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--json" => json = true,
            other => extras.push(other.to_string()),
        }
    }

    // argparse reports every still-missing required positional at once.
    let (Some(artifact_type), Some(output_path)) = (artifact_type.clone(), output_path) else {
        let missing = if artifact_type.is_none() {
            "type, output_path"
        } else {
            "output_path"
        };
        return argparse_error(
            prog,
            &format!("the following arguments are required: {missing}"),
        );
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_new(&NewArgs {
        artifact_type,
        output_path,
        json,
    }) as u8
}

/// `decided init [directory] [--key KEY] [--ticketing PROVIDER] [--profile
/// NAME] [--parent-corpus] [--json]` — order-aware:
/// `--ticketing`/`--profile` are
/// argparse-choice-validated when their VALUE is consumed (an invalid
/// choice beats a later `--version`; an earlier `--version` wins), and a
/// missing option value errors at its own position too.
fn run_init(rest: &[&String]) -> u8 {
    let prog = "decided init";
    let mut directory: Option<String> = None;
    let mut key: String = "RAC".to_string(); // init.DEFAULT_KEY
    let mut key_explicit = false;
    let mut ticketing: Option<String> = None;
    let mut profile: Option<String> = None;
    let mut org_endpoint: Option<String> = None;
    let mut parent_corpus = false;
    let mut json = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    let ticketing_choice = |v: &str| -> Option<u8> {
        if matches!(v, "jira" | "github" | "linear" | "azure-devops" | "servicenow" | "none") {
            None
        } else {
            Some(argparse_error(
                prog,
                &format!(
                    "argument --ticketing: invalid choice: '{v}' (choose from 'jira', 'github', 'linear', 'azure-devops', 'servicenow', 'none')"
                ),
            ))
        }
    };
    let profile_choice = |v: &str| -> Option<u8> {
        if matches!(v, "default" | "enterprise") {
            None
        } else {
            Some(argparse_error(
                prog,
                &format!(
                    "argument --profile: invalid choice: '{v}' (choose from 'default', 'enterprise')"
                ),
            ))
        }
    };

    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            i += 1;
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--version" => {
                skip_usage_record();
                print_stdout(&version_line());
                return 0;
            }
            "-h" | "--help" => {
                skip_usage_record();
                print_stdout(&format!("usage: {prog} ..."));
                return 0;
            }
            "--json" => json = true,
            "--parent-corpus" => parent_corpus = true,
            other if other == "--key" || other.starts_with("--key=") => {
                match take_opt_value(prog, "--key", other, rest, &mut i) {
                    Ok(v) => {
                        key = v;
                        key_explicit = true;
                    }
                    Err(code) => return code,
                }
            }
            other if other == "--ticketing" || other.starts_with("--ticketing=") => {
                match take_opt_value(prog, "--ticketing", other, rest, &mut i) {
                    Ok(v) => {
                        if let Some(code) = ticketing_choice(&v) {
                            return code;
                        }
                        ticketing = Some(v);
                    }
                    Err(code) => return code,
                }
            }
            other if other == "--profile" || other.starts_with("--profile=") => {
                match take_opt_value(prog, "--profile", other, rest, &mut i) {
                    Ok(v) => {
                        if let Some(code) = profile_choice(&v) {
                            return code;
                        }
                        profile = Some(v);
                    }
                    Err(code) => return code,
                }
            }
            other if other == "--org-endpoint" || other.starts_with("--org-endpoint=") => {
                // Free string like --key: the http(s) check is the service
                // layer's `decided:` usage error, not an argparse choice.
                match take_opt_value(prog, "--org-endpoint", other, rest, &mut i) {
                    Ok(v) => org_endpoint = Some(v),
                    Err(code) => return code,
                }
            }
            other => extras.push(other.to_string()),
        }
        i += 1;
    }

    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_init(&InitArgs {
        directory: directory.unwrap_or_else(|| ".".to_string()),
        key,
        key_explicit,
        ticketing,
        profile,
        org_endpoint,
        parent_corpus,
        json,
    }) as u8
}

/// `decided quickstart [directory] [--key KEY] [--type TYPE] [--json]` —
/// order-aware only for the value-taking options: a MISSING `--key`/
/// `--type` value errors at its own position (beating a later
/// `--version`), while their values are free strings validated by the
/// service (so `--key bad --version` prints the version, measured).
fn run_quickstart(rest: &[&String]) -> u8 {
    let prog = "decided quickstart";
    let mut directory: Option<String> = None;
    let mut key: String = "RAC".to_string(); // init.DEFAULT_KEY
    let mut artifact_type: String = "requirement".to_string(); // quickstart.DEFAULT_TYPE
    let mut json = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            i += 1;
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--version" => {
                skip_usage_record();
                print_stdout(&version_line());
                return 0;
            }
            "-h" | "--help" => {
                skip_usage_record();
                print_stdout(&format!("usage: {prog} ..."));
                return 0;
            }
            "--json" => json = true,
            other if other == "--key" || other.starts_with("--key=") => {
                match take_opt_value(prog, "--key", other, rest, &mut i) {
                    Ok(v) => key = v,
                    Err(code) => return code,
                }
            }
            other if other == "--type" || other.starts_with("--type=") => {
                match take_opt_value(prog, "--type", other, rest, &mut i) {
                    Ok(v) => artifact_type = v,
                    Err(code) => return code,
                }
            }
            other => extras.push(other.to_string()),
        }
        i += 1;
    }

    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_quickstart(&QuickstartArgs {
        directory: directory.unwrap_or_else(|| ".".to_string()),
        key,
        artifact_type,
        json,
    }) as u8
}

fn run_rename(rest: &[&String]) -> u8 {
    let prog = "decided rename";
    let mut old: Option<String> = None;
    let mut new: Option<String> = None;
    let mut directory: Option<String> = None;
    let mut apply = false;
    let mut top_level = false;
    let mut json = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    for arg in rest {
        let arg = arg.as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if old.is_none() {
                old = Some(arg.to_string());
            } else if new.is_none() {
                new = Some(arg.to_string());
            } else if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--apply" => apply = true,
            "--top-level" => top_level = true,
            "--json" => json = true,
            other => extras.push(other.to_string()),
        }
    }

    // argparse reports every still-missing required positional at once
    // (positional ORDER is old, new, directory — directory LAST).
    let (Some(old), Some(new), Some(directory)) = (old.clone(), new.clone(), directory) else {
        let mut missing: Vec<&str> = Vec::new();
        if old.is_none() {
            missing.push("old");
        }
        if new.is_none() {
            missing.push("new");
        }
        missing.push("directory");
        return argparse_error(
            prog,
            &format!("the following arguments are required: {}", missing.join(", ")),
        );
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_rename(&RenameArgs {
        old,
        new,
        directory,
        apply,
        top_level,
        json,
    }) as u8
}

/// `decided migrate {metadata} <directory> [--dry-run] [--top-level]
/// [--recursive] [--json]` — order-aware: the `target` positional's choice
/// set is validated when the token is CONSUMED (an invalid target beats a
/// later `--version`; an earlier `--version` wins).
fn run_migrate(rest: &[&String]) -> u8 {
    let prog = "decided migrate";
    let mut target: Option<String> = None;
    let mut directory: Option<String> = None;
    let mut dry_run = false;
    let mut top_level = false;
    let mut json = false;
    let mut extras: Vec<String> = Vec::new();
    let mut positional_only = false;

    for arg in rest {
        let arg = arg.as_str();
        if positional_only || arg == "-" || !arg.starts_with('-') {
            if target.is_none() {
                if arg != "metadata" && arg != "layout" {
                    return argparse_error(
                        prog,
                        &format!(
                            "argument target: invalid choice: '{arg}' (choose from 'metadata', 'layout')"
                        ),
                    );
                }
                target = Some(arg.to_string());
            } else if directory.is_none() {
                directory = Some(arg.to_string());
            } else {
                extras.push(arg.to_string());
            }
            continue;
        }
        match arg {
            "--" => positional_only = true,
            "--version" => {
                skip_usage_record();
                print_stdout(&version_line());
                return 0;
            }
            "-h" | "--help" => {
                skip_usage_record();
                print_stdout(&format!("usage: {prog} ..."));
                return 0;
            }
            "--dry-run" => dry_run = true,
            "--top-level" => top_level = true,
            "--recursive" => {} // affirmation of the default (scope_parent)
            "--json" => json = true,
            other => extras.push(other.to_string()),
        }
    }

    let (Some(target), Some(directory)) = (target.clone(), directory) else {
        let missing = if target.is_none() {
            "target, directory"
        } else {
            "directory"
        };
        return argparse_error(
            prog,
            &format!("the following arguments are required: {missing}"),
        );
    };
    if !extras.is_empty() {
        return unrecognized(&extras);
    }

    cmd_migrate(&MigrateArgs {
        target,
        directory,
        dry_run,
        top_level,
        json,
    }) as u8
}
