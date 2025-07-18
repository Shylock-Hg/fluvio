use sha2::{Digest, Sha256};
use clap::Parser;
use anyhow::Result;

use fluvio::Fluvio;
use fluvio::config::ConfigFile;
use fluvio_cli_common::version_cmd::{FluvioVersionPrinter, os_info};
use fluvio_extension_common::target::ClusterTarget;
use fluvio_channel::FLUVIO_RELEASE_CHANNEL;

use crate::metadata::subcommand_metadata;

#[derive(Debug, Parser)]
pub struct VersionOpt {
    #[clap(short, long)]
    /// Output in JSON format
    pub json: bool,
}

impl VersionOpt {
    pub async fn process(self, target: ClusterTarget) -> Result<()> {
        let mut version_printer = FluvioVersionPrinter::new("Fluvio CLI", crate::VERSION.trim());

        if let Ok(channel_name) = std::env::var(FLUVIO_RELEASE_CHANNEL) {
            version_printer.append_extra("Release Channel", channel_name);
        };

        if let Some(sha) = self.format_frontend_sha() {
            version_printer.append_extra("Fluvio Channel Frontend SHA256", sha);
        }

        let platform = self.format_platform_version(target).await;
        version_printer.append_extra("Fluvio Platform", platform);

        version_printer.append_extra("Git Commit", env!("GIT_HASH"));

        if let Some(info) = os_info() {
            version_printer.append_extra("OS Details", info);
        }

        if self.json {
            println!("{}", version_printer.to_json_pretty()?);
            return Ok(());
        }

        println!("{version_printer}");

        if let Some(metadata) = self.format_subcommand_metadata() {
            if !metadata.is_empty() {
                println!("=== Plugin Versions ===");

                for (name, version) in metadata {
                    self.print_width(&name, &version, 30);
                }
            }
        }

        Ok(())
    }

    fn print_width(&self, name: &str, version: &str, width: usize) {
        println!("{name:width$} : {version}");
    }

    // Read fluvio frontend (fluvio-channel)
    // (assuming it is named `fluvio` alongside a CLI named with its channel name (i.e. fluvio-stable))
    fn format_frontend_sha(&self) -> Option<String> {
        let fluvio_cli = std::env::current_exe().ok()?;
        let mut fluvio_frontend_path = fluvio_cli;
        fluvio_frontend_path.set_file_name("fluvio");

        let fluvio_cli_bin = std::fs::read(fluvio_frontend_path).ok()?;
        let mut hasher = Sha256::new();
        hasher.update(fluvio_cli_bin);
        let fluvio_cli_bin_sha256 = hasher.finalize();
        Some(format!("{:x}", &fluvio_cli_bin_sha256))
    }

    async fn format_platform_version(&self, target: ClusterTarget) -> String {
        // Attempt to connect to a Fluvio cluster to get platform version
        // Even if we fail to connect, we should not fail the other printouts
        let mut platform_version = String::from("Not available");
        if let Ok(fluvio_config) = target.load() {
            if let Ok(fluvio) = Fluvio::connect_with_config(&fluvio_config).await {
                let version = fluvio.platform_version();
                platform_version = version.to_string();
            }
        }

        let profile_name = ConfigFile::load(None)
            .ok()
            .and_then(|it| {
                it.config()
                    .current_profile_name()
                    .map(|name| name.to_string())
            })
            .map(|name| format!(" ({name})"))
            .unwrap_or_default();
        format!("{platform_version}{profile_name}")
    }

    fn format_subcommand_metadata(&self) -> Option<Vec<(String, String)>> {
        let metadata = subcommand_metadata().ok()?;
        let mut formats = Vec::new();
        for cmd in metadata {
            let filename = match cmd.path.file_name() {
                Some(f) => f.to_string_lossy().to_string(),
                None => continue,
            };
            let left = format!("{} ({})", cmd.meta.title, filename);
            formats.push((left, cmd.meta.version.to_string()));
        }

        Some(formats)
    }
}
