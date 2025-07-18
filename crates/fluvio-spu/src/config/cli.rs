//!
//! # CLI for Streaming Processing Unit (SPU)
//!
//! Command line interface to provision SPU id and configure various
//! system parameters.
//!
use std::process;

use anyhow::{anyhow, Result};
use tracing::debug;
use tracing::info;
use clap::Parser;

use fluvio_types::print_cli_err;
use fluvio_types::SpuId;
use fluvio_future::rust_tls::TlsAcceptor;
use fluvio_types::defaults::SPU_PEER_MAX_BYTES;

use super::SpuConfig;

/// cli options
#[derive(Debug, Default, Parser)]
#[command(name = "fluvio-spu", about = "Streaming Processing Unit")]
pub struct SpuOpt {
    /// SPU unique identifier
    #[arg(short = 'i', long = "id", value_name = "integer")]
    pub id: Option<i32>,

    #[arg(short = 'p', long = "public-server", value_name = "host:port")]
    /// Spu server for external communication
    pub bind_public: Option<String>,

    #[arg(short = 'v', long = "private-server", value_name = "host:port")]
    /// Spu server for internal cluster communication
    pub bind_private: Option<String>,

    /// Address of the SC Server
    #[arg(long, value_name = "host:port", env = "FLV_SC_PRIVATE_HOST")]
    pub sc_addr: Option<String>,

    #[arg(long, value_name = "dir", env = "FLV_LOG_BASE_DIR")]
    pub log_base_dir: Option<String>,

    #[arg(long, value_name = "log size", env = "FLV_LOG_SIZE")]
    pub log_size: Option<String>,

    #[arg(long, value_name = "integer", env = "FLV_LOG_INDEX_MAX_BYTES")]
    pub index_max_bytes: Option<u32>,

    #[arg(long, value_name = "integer", env = "FLV_LOG_INDEX_MAX_INTERVAL_BYTES")]
    pub index_max_interval_bytes: Option<u32>,

    /// max bytes to transfer between leader and follower
    #[arg(
        long,
        value_name = "integer",
        env = "FLV_PEER_MAX_BYTES",
        default_value_t = SPU_PEER_MAX_BYTES
    )]
    pub peer_max_bytes: u32,

    #[arg(
        long,
        value_name = "integer",
        env = "FLV_SMART_ENGINE_MAX_MEMORY_BYTES"
    )]
    pub smart_engine_max_memory: Option<usize>,

    #[clap(flatten)]
    tls: TlsConfig,
}

impl SpuOpt {
    /// Validate SPU (Streaming Processing Unit) cli inputs and generate SpuConfig
    fn get_spu_config(self) -> Result<(SpuConfig, Option<(TlsAcceptor, String)>)> {
        let tls_acceptor = self.try_build_tls_acceptor()?;
        let (spu_config, tls_addr_opt) = self.as_spu_config()?;
        let tls_config = tls_acceptor.map(|it| (it, tls_addr_opt.unwrap()));
        Ok((spu_config, tls_config))
    }

    #[allow(clippy::wrong_self_convention)]
    fn as_spu_config(self) -> Result<(SpuConfig, Option<String>)> {
        use std::path::PathBuf;

        let mut config = SpuConfig {
            id: match self.id {
                Some(id) => id,
                None => {
                    debug!("no id passed as argument, searching in the env");
                    find_spu_id_from_env()?
                }
            },
            ..Default::default()
        };

        if let Some(sc_endpoint) = self.sc_addr {
            info!("using sc endpoint from env var: {}", sc_endpoint);
            config.sc_endpoint = sc_endpoint;
        }

        if let Some(log_base) = self.log_base_dir {
            info!("overriding log base: {}", log_base);
            config.log.base_dir = PathBuf::from(log_base);
        }

        if let Some(log_size) = self.log_size {
            info!("overriding log size {}", log_size);
            config.log.size = log_size;
        }

        if let Some(index_max_bytes) = self.index_max_bytes {
            info!("overriding index max bytes: {}", index_max_bytes);
            config.log.index_max_bytes = index_max_bytes;
        }

        if let Some(index_max_interval_bytes) = self.index_max_interval_bytes {
            info!("overriding index max bytes: {}", index_max_interval_bytes);
            config.log.index_max_interval_bytes = index_max_interval_bytes;
        }

        if let Some(public_addr) = self.bind_public {
            info!("overriding public addr: {}", public_addr);
            config.public_endpoint = public_addr;
        }

        let mut tls_port: Option<String> = None;

        if self.tls.tls {
            let proxy_addr = config.public_endpoint.clone();
            debug!("using tls proxy addr: {}", proxy_addr);
            tls_port = Some(proxy_addr);
            config.public_endpoint = self
                .tls
                .bind_non_tls_public
                .ok_or_else(|| anyhow!("non tls addr for public must be specified"))?;
        }

        if let Some(private_addr) = self.bind_private {
            info!("overriding private addr: {}", private_addr);
            config.private_endpoint = private_addr;
        }

        config.peer_max_bytes = self.peer_max_bytes;

        if let Some(smart_engine_max_memory) = self.smart_engine_max_memory {
            info!(
                "overriding smart engine max memory: {}",
                smart_engine_max_memory
            );
            config.smart_engine.store_max_memory = smart_engine_max_memory;
        }

        Ok((config, tls_port))
    }

    fn try_build_tls_acceptor(&self) -> Result<Option<TlsAcceptor>> {
        let tls_config = &self.tls;
        if !tls_config.tls {
            return Ok(None);
        }

        let server_crt_path = tls_config
            .server_cert
            .as_ref()
            .ok_or_else(|| anyhow!("missing server cert"))?;
        let server_key_path = tls_config
            .server_key
            .as_ref()
            .ok_or_else(|| anyhow!("missing server key"))?;

        let acceptor = if tls_config.enable_client_cert {
            let ca_path = tls_config
                .ca_cert
                .as_ref()
                .ok_or_else(|| anyhow!("missing ca cert"))?;
            fluvio_future::rust_tls::AcceptorBuilder::with_safe_defaults()
                .client_authenticate(ca_path)?
                .load_server_certs(server_crt_path, server_key_path)?
                .build()
        } else {
            fluvio_future::rust_tls::AcceptorBuilder::with_safe_defaults()
                .no_client_authentication()
                .load_server_certs(server_crt_path, server_key_path)?
                .build()
        };

        Ok(Some(acceptor))
    }

    pub fn process_spu_cli_or_exit(self) -> (SpuConfig, Option<(TlsAcceptor, String)>) {
        match self.get_spu_config() {
            Err(err) => {
                print_cli_err!(err);
                process::exit(-1);
            }
            Ok(config) => config,
        }
    }
}

/// find spu id from env, if not found, return error
fn find_spu_id_from_env() -> Result<SpuId> {
    use std::env;
    use fluvio_types::defaults::FLV_SPU_ID;

    if let Ok(id_str) = env::var(FLV_SPU_ID) {
        debug!("found spu id from env: {}", id_str);
        let id = id_str.parse().map_err(|err| anyhow!("spu-id: {err}"))?;
        Ok(id)
    } else {
        // try get special env SPU which has form of {}-{id} when in as in-cluster config
        if let Ok(spu_name) = env::var("SPU_INDEX") {
            info!("extracting SPU from: {}", spu_name);
            let spu_tokens: Vec<&str> = spu_name.split('-').collect();
            if spu_tokens.len() < 2 {
                Err(anyhow!("SPU is invalid format: {spu_name} bailing out"))
            } else {
                let spu_token = spu_tokens[spu_tokens.len() - 1];
                let id: SpuId = spu_token
                    .parse()
                    .map_err(|err| anyhow!("invalid spu id: {err}"))?;
                info!("found SPU INDEX ID: {}", id);

                // now we get SPU_MIN which tells min
                let spu_min_var = env::var("SPU_MIN").unwrap_or_else(|_| "0".to_owned());
                debug!("found SPU MIN ID: {}", spu_min_var);
                let base_id: SpuId = spu_min_var
                    .parse()
                    .map_err(|err| anyhow!("invalid spu min id: {err}"))?;
                Ok(id + base_id)
            }
        } else {
            Err(anyhow!(
                "SPU index id not found from SPU_INDEX or {FLV_SPU_ID}"
            ))
        }
    }
}

/// same in the SC
#[derive(Debug, Parser, Default)]
struct TlsConfig {
    /// enable tls
    #[arg(long)]
    pub tls: bool,

    /// TLS: path to server certificate
    #[arg(long)]
    pub server_cert: Option<String>,
    #[arg(long)]
    /// TLS: path to server private key
    pub server_key: Option<String>,
    /// TLS: enable client cert
    #[arg(long)]
    pub enable_client_cert: bool,
    /// TLS: path to ca cert, required when client cert is enabled
    #[arg(long)]
    pub ca_cert: Option<String>,

    #[arg(long)]
    /// TLS: address of non tls public service, required
    pub bind_non_tls_public: Option<String>,
}
