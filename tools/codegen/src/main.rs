// SPDX-License-Identifier: MulanPSL-2.0
// robonix-codegen — ROS IDL + contract codegen

use anyhow::{Result, bail};
use clap::Parser;
use std::collections::BTreeSet;
use std::io::{self, IsTerminal};
use std::path::PathBuf;

use robonix_codegen::codegen::{
    contract_gen, docs_gen, mcp_python_gen, msg_parser, proto_gen, ros2_gen,
    zc_python_gen, zc_scanner,
};
use robonix_scribe::info;

#[derive(Parser)]
#[command(name = "robonix-codegen")]
#[command(
    about = "ROS IDL + contract TOML codegen: .msg/.srv → .proto / MCP-Python; contracts → robonix_contracts.proto"
)]
struct Args {
    /// Type search path for ROS IDL (e.g. -I path/to/robonix-interfaces/lib). Optional if only `--contracts` with protobuf-only types (rare).
    #[arg(short = 'I', long = "include", number_of_values = 1)]
    include: Vec<PathBuf>,

    /// One or more directories of contract `.toml` files (e.g.
    /// <root>/capabilities and <pkg>/capabilities). Pass multiple times
    /// to merge per-package contracts with the global tree. Emits
    /// `robonix_contracts.proto` (proto only).
    #[arg(long = "contracts", number_of_values = 1)]
    contracts: Vec<PathBuf>,

    /// Output directory for generated code
    #[arg(short = 'o', long = "out")]
    out: PathBuf,

    /// Target language: "proto" (gRPC stubs + contracts), "mcp"
    /// (Python typed-input helpers for MCP servers), "ros2"
    /// (a colcon overlay of the canonical ROS interface packages),
    /// "ros2_zc" (ZC contract TOMLs + Python provider skeletons), or
    /// "docs" (the mdBook contract + ROS IDL reference pages).
    #[arg(long = "lang", default_value = "proto")]
    lang: String,

    /// For `--lang docs` only: version line written verbatim into the
    /// generated reference headers (e.g. "v0.1 @ abc1234 (2026-06-06)").
    #[arg(long = "doc-stamp")]
    doc_stamp: Option<String>,

    /// For `--lang ros2_zc` only: one or more directories containing
    /// existing (non-ZC) provider source code.  Codegen scans these for
    /// `Primitive(...)` / namespace declarations, matches them to the
    /// generated ZC skeletons, and emits a `fill_manifest.json` that
    /// maps each skeleton to its reference implementation.
    #[arg(long = "fill", number_of_values = 1)]
    fill: Vec<PathBuf>,

    /// For `--lang ros2_zc` only: only process contracts matching this
    /// prefix or exact id.  Pass multiple times for multiple matches.
    /// Example: `--filter robonix/primitive/camera/rgb` for one contract,
    /// or `--filter robonix/primitive/lidar` for all lidar contracts.
    /// Without `--filter`, all heavy-payload contracts are processed.
    #[arg(long = "filter", number_of_values = 1)]
    filter: Vec<String>,

    /// Print per-package lines and every IDL resolution warning (default: one summary line; set ROBONIX_CODEGEN_VERBOSE=1 for same without flag)
    #[arg(long, short = 'v')]
    verbose: bool,
}

fn ridlc_prefix() -> &'static str {
    if io::stderr().is_terminal() {
        "\x1b[1;38;5;45m[robonix-codegen]\x1b[0m"
    } else {
        "[robonix-codegen]"
    }
}

fn verbose_from_args_and_env(args: &Args) -> bool {
    if args.verbose {
        return true;
    }
    std::env::var_os("ROBONIX_CODEGEN_VERBOSE").is_some_and(|v| {
        let s = v.to_string_lossy();
        s == "1" || s.eq_ignore_ascii_case("true")
    })
}

fn main() -> Result<()> {
    robonix_scribe::init("codegen");
    info!("robonix-codegen starting");
    let args = Args::parse();
    let verbose = verbose_from_args_and_env(&args);

    if args.include.is_empty() && args.contracts.is_empty() {
        bail!("[robonix-codegen] pass at least one -I/--include and/or --contracts (see --help)");
    }

    match args.lang.as_str() {
        "proto" | "mcp" | "ros2" | "ros2_zc" | "docs" => {}
        other => {
            bail!(
                "[robonix-codegen] unsupported --lang '{other}'. Supported: 'proto', 'mcp', 'ros2', 'ros2_zc', 'docs'."
            )
        }
    }

    if verbose {
        eprintln!("{} includes: {:?}", ridlc_prefix(), args.include);
        eprintln!(
            "{} contracts: {:?}",
            ridlc_prefix(),
            args.contracts
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
        );
        eprintln!("{} out: {}", ridlc_prefix(), args.out.display());
    }

    std::fs::create_dir_all(&args.out)?;

    // Docs reference: a pure file walk over contracts + lib IDL — no
    // resolver / proto build needed. Handle before building the resolver
    // so an unrelated task doesn't emit IDL-resolution warnings.
    if args.lang == "docs" {
        docs_gen::generate(
            &args.contracts,
            &args.include,
            &args.out,
            args.doc_stamp.as_deref(),
            verbose,
        )?;
        return Ok(());
    }

    // ros2_zc: scan contracts for heavy-payload types and emit ZC
    // contract TOMLs + Python provider skeletons.  Like docs, this
    // only needs the contracts dirs — no IDL resolver required.
    if args.lang == "ros2_zc" {
        if args.contracts.is_empty() {
            bail!("[robonix-codegen] ros2_zc: pass at least one --contracts <dir>");
        }
        let mut candidates = zc_scanner::scan_contracts(&args.contracts)?;

        // --filter is required: the developer decides which contracts to upgrade.
        if args.filter.is_empty() {
            bail!(
                "[robonix-codegen] ros2_zc: --filter is required.\n\
                 Pass one or more contract id prefixes, e.g.:\n\
                   --filter robonix/primitive/camera/rgb    (one contract)\n\
                   --filter robonix/primitive/lidar          (all lidar)\n\
                   --filter robonix/primitive/camera/rgb --filter robonix/primitive/lidar/lidar3d"
            );
        }

        let filters: Vec<&str> = args.filter.iter().map(|s| s.as_str()).collect();
        candidates.retain(|c| {
            filters
                .iter()
                .any(|f| c.contract_id.starts_with(f))
        });
        if candidates.is_empty() {
            eprintln!(
                "{} ros2_zc: no contracts match filter(s): {:?}",
                ridlc_prefix(),
                filters
            );
            return Ok(());
        }

        eprintln!(
            "{} ros2_zc: {} candidates found",
            ridlc_prefix(),
            candidates.len()
        );
        let fill_providers = if args.fill.is_empty() {
            Vec::new()
        } else {
            zc_scanner::discover_providers(&args.fill)?
        };
        zc_python_gen::generate(&candidates, &args.out, &fill_providers, &args.fill, verbose)?;
        return Ok(());
    }

    let mut resolver = msg_parser::MsgResolver::new(&args.include)?;
    let mut idl_skips = 0usize;
    resolver.resolve_all_in_index(verbose, &mut idl_skips)?;
    resolver.resolve_all_srv(verbose, &mut idl_skips)?;

    if !verbose && idl_skips > 0 {
        eprintln!(
            "{} {} IDL msg/srv skipped (unresolved deps); use --verbose for details",
            ridlc_prefix(),
            idl_skips
        );
    }

    let contract_srvs: Option<BTreeSet<(String, String)>> = if args.contracts.is_empty() {
        None
    } else {
        let mut set = BTreeSet::new();
        for cdir in &args.contracts {
            for pair in contract_gen::collect_referenced_srvs(cdir)? {
                set.insert(pair);
            }
        }
        Some(set)
    };

    let specs = resolver.ordered_specs();
    let srvs = resolver.ordered_srvs();
    let mut pkgs = std::collections::BTreeSet::new();
    for s in &specs {
        pkgs.insert(&s.package);
    }
    for s in &srvs {
        pkgs.insert(&s.package);
    }

    match args.lang.as_str() {
        "mcp" => {
            mcp_python_gen::generate(&resolver, &args.out, verbose)?;
            eprintln!(
                "{} mcp: {} packages, {} msgs -> {}",
                ridlc_prefix(),
                pkgs.len(),
                specs.len(),
                args.out.display()
            );
        }
        "ros2" => {
            ros2_gen::generate(&resolver, &args.out, verbose)?;
        }
        _ => {
            proto_gen::generate(&resolver, &args.out, contract_srvs.as_ref(), verbose)?;
            eprintln!(
                "{} proto: {} packages, {} msgs, {} srv -> {}",
                ridlc_prefix(),
                pkgs.len(),
                specs.len(),
                srvs.len(),
                args.out.display()
            );
            if !args.contracts.is_empty() {
                contract_gen::generate(&mut resolver, &args.contracts, &args.out, verbose)?;
                if !verbose {
                    eprintln!(
                        "{} contracts: robonix_contracts.proto + contract_proto_modules.rs (under {})",
                        ridlc_prefix(),
                        args.out.display()
                    );
                }
            }
        }
    }

    Ok(())
}
