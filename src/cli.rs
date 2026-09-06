use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

const ROOT_USAGE: &str = "cargo teaql [OPTIONS] [COMMAND]\n    cargo teaql [OPTIONS] <TARGET> --input <MODEL> [--output <DIR>]\n    cargo teaql [OPTIONS] <LANG>-assist-<ACTION>/<ENTITY>[.<FIELD>] --input <MODEL>";

const ROOT_EXAMPLES: &str = r#"REMOTE WORKFLOWS (dynamic commands):
  Discover the targets and versions currently provided by the TeaQL service:
    cargo teaql services
    cargo teaql version

  Evaluate a KSML model before generation:
    cargo teaql evaluate --input models/

  Generate a typed library or runnable workspace:
    cargo teaql rust-lib-core --input models/ --output build/
    cargo teaql typescript-app-console --input models/ --output build/

  Ask model-aware Assist about an entity or one field:
    cargo teaql rust-assist-query/school --input models/
    cargo teaql rust-assist-query/school.established_date --input models/

Run `cargo teaql services` for the current remote target inventory.
With no command, TeaQL runs `services`. `cargo-teaql` is an equivalent invocation."#;

#[derive(Debug, Parser)]
#[command(
    name = "teaql",
    version,
    about = "Evaluate TeaQL models, generate typed applications, and discover model-aware APIs",
    long_about = "TeaQL turns a KSML semantic model into evaluated, language-native libraries and runnable workspaces. It also provides progressive, model-aware Assist for discovering the generated API without reading generated source.",
    override_usage = ROOT_USAGE,
    after_help = ROOT_EXAMPLES,
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Workspace directory used by local commands and relative output paths.
    #[arg(long, global = true, default_value = ".")]
    pub cwd: PathBuf,

    /// Show configuration sources and request progress.
    #[arg(long, short, global = true)]
    pub verbose: bool,

    /// Show additional diagnostic details.
    #[arg(long, global = true)]
    pub debug: bool,

    /// KSML model file or directory; multi-file models use the directory containing main.xml.
    #[arg(long, global = true)]
    pub input: Option<PathBuf>,

    /// TeaQL service endpoint prefix, for example https://api.teaql.io/latest/.
    #[arg(long, global = true)]
    pub endpoint_prefix: Option<String>,

    /// Deprecated alias for --endpoint-prefix.
    #[arg(long, global = true)]
    #[arg(hide = true)]
    pub service_url: Option<String>,

    /// TeaQL API key; the built-in free-tier key is used by default.
    #[arg(long, global = true)]
    pub api_key: Option<String>,

    /// Generated output directory; relative paths are resolved from --cwd.
    #[arg(long, global = true)]
    pub output: Option<PathBuf>,

    /// Remote request timeout in seconds.
    #[arg(long, global = true)]
    pub timeout_seconds: Option<u64>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Show the saved workspace configuration and its file path.
    ShowConfig,
    /// Interactively configure TeaQL for the current workspace.
    Config,
    /// Verify service connectivity with a built-in model round trip.
    Ping(ServiceArgs),
    /// Install executable aliases used by cargo-style invocation.
    InstallLinks(InstallLinksArgs),
    /// Run cargo check and map any compiler errors back to the source KSML (XML) file.
    Check(CheckArgs),

    #[command(external_subcommand)]
    Dynamic(Vec<OsString>),
}

#[derive(Debug, Parser)]
#[command(no_binary_name = true)]
pub struct DynamicArgs {
    /// Additional path segments for the dynamic endpoint
    #[arg(trailing_var_arg = true, allow_hyphen_values = false)]
    pub paths: Vec<String>,

    /// Override the working directory.
    #[arg(long)]
    pub cwd: Option<PathBuf>,

    /// The input model file or directory (defaults to current directory if not specified)
    #[arg(long)]
    pub input: Option<PathBuf>,

    /// Override TeaQL endpoint prefix.
    #[arg(long)]
    pub endpoint_prefix: Option<String>,

    /// Override TeaQL service URL. Deprecated: use --endpoint-prefix.
    #[arg(long)]
    pub service_url: Option<String>,

    /// Override API Key.
    #[arg(long)]
    pub api_key: Option<String>,

    /// Override output directory.
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Override request timeout in seconds.
    #[arg(long)]
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    /// Pass additional arguments to cargo check (e.g. --workspace, --tests).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub cargo_args: Vec<String>,
}

#[derive(Debug, Args)]
pub struct GenerateArgs {
    /// Model file or directory to upload.
    pub input: PathBuf,

    /// Override TeaQL endpoint prefix, for example https://api.teaql.io/latest/.
    #[arg(long)]
    pub endpoint_prefix: Option<String>,

    /// Override TeaQL service URL. Deprecated: use --endpoint-prefix.
    #[arg(long)]
    pub service_url: Option<String>,

    /// Override API Key. (Default: Built-in free tier OOTB key)
    #[arg(long)]
    pub api_key: Option<String>,

    /// Override output directory.
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Override request timeout in seconds.
    #[arg(long)]
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Args)]
pub struct GenServiceArgs {
    #[command(flatten)]
    pub generate_args: GenerateArgs,

    /// The target service to generate (e.g. rust-app-console)
    #[arg(long, short = 's')]
    pub service: String,
}

#[derive(Debug, Args)]
pub struct ServiceArgs {
    /// Override TeaQL endpoint prefix, for example https://api.teaql.io/latest/.
    #[arg(long)]
    pub endpoint_prefix: Option<String>,

    /// Override TeaQL service URL. Deprecated: use --endpoint-prefix.
    #[arg(long)]
    pub service_url: Option<String>,

    /// Override API Key. (Default: Built-in free tier OOTB key)
    #[arg(long)]
    pub api_key: Option<String>,

    /// Override request timeout in seconds.
    #[arg(long)]
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Args)]
pub struct InstallLinksArgs {
    /// Directory where symlinks should be created. Defaults to the current executable directory.
    #[arg(long)]
    pub dir: Option<PathBuf>,

    /// Replace existing files or symlinks when needed.
    #[arg(long)]
    pub force: bool,
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::Cli;

    #[test]
    fn root_help_exposes_primary_workflows() {
        let help = Cli::command().render_long_help().to_string();

        assert!(help.contains("cargo teaql evaluate --input models/"));
        assert!(help.contains("cargo teaql rust-lib-core --input models/"));
        assert!(help.contains("rust-assist-query/school.established_date"));
        assert!(help.contains("cargo teaql services"));
    }

    #[test]
    fn root_help_explains_global_options_without_deprecated_alias() {
        let help = Cli::command().render_long_help().to_string();

        assert!(help.contains("KSML model file or directory"));
        assert!(help.contains("relative paths are resolved from --cwd"));
        assert!(!help.contains("--service-url"));
    }
}
