use std::collections::BTreeMap;

use anyhow::{bail, Context};

use sozu_command_lib::{
    certificate::{
        calculate_fingerprint, split_certificate_chain, CertificateAndKey, Fingerprint, TlsVersion,
    },
    config::{Config, ListenerBuilder, ProxyProtocolConfig},
    proto::command::{FrontendFilters, PathRule, RequestHttpFrontend, RulePosition},
    request::{
        ActivateListener, AddBackend, AddCertificate, Cluster, DeactivateListener, ListenerType,
        LoadBalancingParams, MetricsConfiguration, RemoveBackend, RemoveCertificate,
        RemoveListener, ReplaceCertificate, Request, RequestTcpFrontend,
    },
};

use crate::{
    cli::{
        BackendCmd, ClusterCmd, HttpFrontendCmd, HttpListenerCmd, HttpsListenerCmd, LoggingLevel,
        MetricsCmd, TcpFrontendCmd, TcpListenerCmd,
    },
    ctl::CommandManager,
};

impl CommandManager {
    pub fn save_state(&mut self, path: String) -> anyhow::Result<()> {
        println!("Loading the state to file {path}");

        self.order_request(Request::SaveState { path })
    }

    pub fn load_state(&mut self, path: String) -> anyhow::Result<()> {
        println!("Loading the state on path {path}");

        self.order_request(Request::LoadState { path })
    }

    pub fn dump_state(&mut self, json: bool) -> anyhow::Result<()> {
        println!("Dumping the state, json={json}");

        self.order_request_to_all_workers(Request::DumpState, json)
    }

    pub fn soft_stop(&mut self) -> anyhow::Result<()> {
        println!("shutting down proxy softly");

        self.order_request_to_all_workers(Request::SoftStop, false)
    }

    pub fn hard_stop(&mut self) -> anyhow::Result<()> {
        println!("shutting down proxy the hard way");

        self.order_request_to_all_workers(Request::HardStop, false)
    }
    /*
    pub fn upgrade_worker(&mut self, worker_id: u32) -> anyhow::Result<()> {
        println!("upgrading worker {}", worker_id);

        self.order_command_with_worker_id(
            CommandRequestOrder::UpgradeWorker(worker_id),
            Some(worker_id),
            false,
        )
    }
    */

    pub fn status(&mut self, json: bool) -> anyhow::Result<()> {
        println!("Requesting status…");

        self.order_request_to_all_workers(Request::Status, json)
    }

    pub fn configure_metrics(&mut self, cmd: MetricsCmd) -> anyhow::Result<()> {
        println!("Configuring metrics: {cmd:?}");

        let configuration = match cmd {
            MetricsCmd::Enable => MetricsConfiguration::Enabled,
            MetricsCmd::Disable => MetricsConfiguration::Disabled,
            MetricsCmd::Clear => MetricsConfiguration::Clear,
            _ => bail!("The command passed to the configure_metrics function is wrong."),
        };

        self.order_request(Request::ConfigureMetrics(configuration))
    }

    pub fn reload_configuration(&mut self, path: Option<String>, json: bool) -> anyhow::Result<()> {
        println!("Reloading configuration…");

        self.order_request_to_all_workers(Request::ReloadConfiguration { path }, json)
    }

    pub fn list_frontends(
        &mut self,
        http: bool,
        https: bool,
        tcp: bool,
        domain: Option<String>,
    ) -> anyhow::Result<()> {
        println!("Listing frontends");

        self.order_request(Request::ListFrontends(FrontendFilters {
            http,
            https,
            tcp,
            domain,
        }))
    }

    pub fn events(&mut self) -> anyhow::Result<()> {
        self.order_request(Request::SubscribeEvents)
    }

    pub fn backend_command(&mut self, cmd: BackendCmd) -> anyhow::Result<()> {
        match cmd {
            BackendCmd::Add {
                id,
                backend_id,
                address,
                sticky_id,
                backup,
            } => self.order_request(Request::AddBackend(AddBackend {
                cluster_id: id,
                address: address.to_string(),
                backend_id,
                load_balancing_parameters: Some(LoadBalancingParams::default()),
                sticky_id,
                backup,
            })),
            BackendCmd::Remove {
                id,
                backend_id,
                address,
            } => self.order_request(Request::RemoveBackend(RemoveBackend {
                cluster_id: id,
                address: address.to_string(),
                backend_id,
            })),
        }
    }

    pub fn cluster_command(&mut self, cmd: ClusterCmd) -> anyhow::Result<()> {
        match cmd {
            ClusterCmd::Add {
                id,
                sticky_session,
                https_redirect,
                send_proxy,
                expect_proxy,
                load_balancing_policy,
            } => {
                let proxy_protocol = match (send_proxy, expect_proxy) {
                    (true, true) => Some(ProxyProtocolConfig::RelayHeader),
                    (true, false) => Some(ProxyProtocolConfig::SendHeader),
                    (false, true) => Some(ProxyProtocolConfig::ExpectHeader),
                    _ => None,
                };
                self.order_request(Request::AddCluster(Cluster {
                    cluster_id: id,
                    sticky_session,
                    https_redirect,
                    proxy_protocol,
                    load_balancing: load_balancing_policy,
                    load_metric: None,
                    answer_503: None,
                }))
            }
            ClusterCmd::Remove { id } => {
                self.order_request(Request::RemoveCluster { cluster_id: id })
            }
        }
    }

    pub fn tcp_frontend_command(&mut self, cmd: TcpFrontendCmd) -> anyhow::Result<()> {
        match cmd {
            TcpFrontendCmd::Add { id, address, tags } => {
                self.order_request(Request::AddTcpFrontend(RequestTcpFrontend {
                    cluster_id: id,
                    address: address.to_string(),
                    tags,
                }))
            }
            TcpFrontendCmd::Remove { id, address } => {
                self.order_request(Request::RemoveTcpFrontend(RequestTcpFrontend {
                    cluster_id: id,
                    address: address.to_string(),
                    tags: None,
                }))
            }
        }
    }

    pub fn http_frontend_command(&mut self, cmd: HttpFrontendCmd) -> anyhow::Result<()> {
        match cmd {
            HttpFrontendCmd::Add {
                hostname,
                path_prefix,
                path_regex,
                path_equals,
                address,
                method,
                cluster_id: route,
                tags,
            } => self.order_request(Request::AddHttpFrontend(RequestHttpFrontend {
                cluster_id: route.into(),
                address: address.to_string(),
                hostname,
                path: PathRule::from_cli_options(path_prefix, path_regex, path_equals),
                method: method.map(String::from),
                position: RulePosition::Tree.into(),
                tags: match tags {
                    Some(tags) => tags,
                    None => BTreeMap::new(),
                },
            })),

            HttpFrontendCmd::Remove {
                hostname,
                path_prefix,
                path_regex,
                path_equals,
                address,
                method,
                cluster_id: route,
            } => self.order_request(Request::RemoveHttpFrontend(RequestHttpFrontend {
                cluster_id: route.into(),
                address: address.to_string(),
                hostname,
                path: PathRule::from_cli_options(path_prefix, path_regex, path_equals),
                method: method.map(String::from),
                position: RulePosition::Tree.into(),
                tags: BTreeMap::new(),
            })),
        }
    }

    pub fn https_frontend_command(&mut self, cmd: HttpFrontendCmd) -> anyhow::Result<()> {
        match cmd {
            HttpFrontendCmd::Add {
                hostname,
                path_prefix,
                path_regex,
                path_equals,
                address,
                method,
                cluster_id: route,
                tags,
            } => self.order_request(Request::AddHttpsFrontend(RequestHttpFrontend {
                cluster_id: route.into(),
                address: address.to_string(),
                hostname,
                path: PathRule::from_cli_options(path_prefix, path_regex, path_equals),
                method: method.map(String::from),
                position: RulePosition::Tree.into(),
                tags: match tags {
                    Some(tags) => tags,
                    None => BTreeMap::new(),
                },
            })),
            HttpFrontendCmd::Remove {
                hostname,
                path_prefix,
                path_regex,
                path_equals,
                address,
                method,
                cluster_id: route,
            } => self.order_request(Request::RemoveHttpsFrontend(RequestHttpFrontend {
                cluster_id: route.into(),
                address: address.to_string(),
                hostname,
                path: PathRule::from_cli_options(path_prefix, path_regex, path_equals),
                method: method.map(String::from),
                position: RulePosition::Tree.into(),
                tags: BTreeMap::new(),
            })),
        }
    }

    pub fn https_listener_command(&mut self, cmd: HttpsListenerCmd) -> anyhow::Result<()> {
        match cmd {
            HttpsListenerCmd::Add {
                address,
                public_address,
                answer_404,
                answer_503,
                tls_versions,
                cipher_list,
                expect_proxy,
                sticky_name,
                front_timeout,
                back_timeout,
                request_timeout,
                connect_timeout,
            } => {
                let https_listener = ListenerBuilder::new_https(address)
                    .with_public_address(public_address)
                    .with_answer_404_path(answer_404)
                    .with_answer_503_path(answer_503)
                    .with_tls_versions(tls_versions)
                    .with_cipher_list(cipher_list)
                    .with_expect_proxy(expect_proxy)
                    .with_sticky_name(sticky_name)
                    .with_front_timeout(front_timeout)
                    .with_back_timeout(back_timeout)
                    .with_request_timeout(request_timeout)
                    .with_connect_timeout(connect_timeout)
                    .to_tls()
                    .with_context(|| "Error creating HTTPS listener")?;

                self.order_request(Request::AddHttpsListener(https_listener))
            }
            HttpsListenerCmd::Remove { address } => {
                self.remove_listener(address.to_string(), ListenerType::HTTPS)
            }
            HttpsListenerCmd::Activate { address } => {
                self.activate_listener(address.to_string(), ListenerType::HTTPS)
            }
            HttpsListenerCmd::Deactivate { address } => {
                self.deactivate_listener(address.to_string(), ListenerType::HTTPS)
            }
        }
    }

    pub fn http_listener_command(&mut self, cmd: HttpListenerCmd) -> anyhow::Result<()> {
        match cmd {
            HttpListenerCmd::Add {
                address,
                public_address,
                answer_404,
                answer_503,
                expect_proxy,
                sticky_name,
                front_timeout,
                back_timeout,
                request_timeout,
                connect_timeout,
            } => {
                let http_listener = ListenerBuilder::new_http(address)
                    .with_public_address(public_address)
                    .with_answer_404_path(answer_404)
                    .with_answer_503_path(answer_503)
                    .with_expect_proxy(expect_proxy)
                    .with_sticky_name(sticky_name)
                    .with_front_timeout(front_timeout)
                    .with_request_timeout(request_timeout)
                    .with_back_timeout(back_timeout)
                    .with_connect_timeout(connect_timeout)
                    .to_http()
                    .with_context(|| "Error creating HTTP listener")?;
                self.order_request(Request::AddHttpListener(http_listener))
            }
            HttpListenerCmd::Remove { address } => {
                self.remove_listener(address.to_string(), ListenerType::HTTP)
            }
            HttpListenerCmd::Activate { address } => {
                self.activate_listener(address.to_string(), ListenerType::HTTP)
            }
            HttpListenerCmd::Deactivate { address } => {
                self.deactivate_listener(address.to_string(), ListenerType::HTTP)
            }
        }
    }

    pub fn tcp_listener_command(&mut self, cmd: TcpListenerCmd) -> anyhow::Result<()> {
        match cmd {
            TcpListenerCmd::Add {
                address,
                public_address,
                expect_proxy,
            } => {
                let listener = ListenerBuilder::new_tcp(address)
                    .with_public_address(public_address)
                    .with_expect_proxy(expect_proxy)
                    .to_tcp()
                    .with_context(|| "Could not create TCP listener")?;

                self.order_request(Request::AddTcpListener(listener))
            }
            TcpListenerCmd::Remove { address } => {
                self.remove_listener(address.to_string(), ListenerType::TCP)
            }
            TcpListenerCmd::Activate { address } => {
                self.activate_listener(address.to_string(), ListenerType::TCP)
            }
            TcpListenerCmd::Deactivate { address } => {
                self.deactivate_listener(address.to_string(), ListenerType::TCP)
            }
        }
    }

    pub fn list_listeners(&mut self) -> anyhow::Result<()> {
        self.order_request(Request::ListListeners)
    }

    pub fn remove_listener(&mut self, address: String, proxy: ListenerType) -> anyhow::Result<()> {
        self.order_request(Request::RemoveListener(RemoveListener {
            address: address.parse().with_context(|| "wrong socket address")?,
            proxy,
        }))
    }

    pub fn activate_listener(
        &mut self,
        address: String,
        proxy: ListenerType,
    ) -> anyhow::Result<()> {
        self.order_request(Request::ActivateListener(ActivateListener {
            address: address.parse().with_context(|| "wrong socket address")?,
            proxy,
            from_scm: false,
        }))
    }

    pub fn deactivate_listener(
        &mut self,
        address: String,
        proxy: ListenerType,
    ) -> anyhow::Result<()> {
        self.order_request(Request::DeactivateListener(DeactivateListener {
            // address,
            address: address.parse().with_context(|| "wrong socket address")?,
            proxy,
            to_scm: false,
        }))
    }

    pub fn logging_filter(&mut self, filter: &LoggingLevel) -> anyhow::Result<()> {
        self.order_request(Request::Logging(filter.to_string().to_lowercase()))
    }

    pub fn add_certificate(
        &mut self,
        address: String,
        certificate_path: &str,
        certificate_chain_path: &str,
        key_path: &str,
        versions: Vec<TlsVersion>,
    ) -> anyhow::Result<()> {
        let new_certificate =
            load_full_certificate(certificate_path, certificate_chain_path, key_path, versions)
                .with_context(|| "Could not load the full certificate")?;

        self.order_request(Request::AddCertificate(AddCertificate {
            address,
            certificate: new_certificate,
            names: vec![],
            expired_at: None,
        }))
    }

    pub fn replace_certificate(
        &mut self,
        address: String,
        new_certificate_path: &str,
        new_certificate_chain_path: &str,
        new_key_path: &str,
        old_certificate_path: Option<&str>,
        old_fingerprint: Option<&str>,
        versions: Vec<TlsVersion>,
    ) -> anyhow::Result<()> {
        let old_fingerprint = match (old_certificate_path, old_fingerprint) {
            (None, None) | (Some(_), Some(_)) => {
                bail!("Error: Please provide either one, the old certificate's path OR its fingerprint")
            }
            (Some(old_certificate_path), None) => {
                get_fingerprint_from_certificate_path(old_certificate_path).with_context(|| {
                    "Could not retrieve the fingerprint from the given certificate path"
                })?
            }
            (None, Some(fingerprint)) => decode_fingerprint(fingerprint)
                .with_context(|| "Error decoding the given fingerprint")?,
        };

        let new_certificate = load_full_certificate(
            new_certificate_path,
            new_certificate_chain_path,
            new_key_path,
            versions,
        )
        .with_context(|| "Could not load the full certificate")?;

        self.order_request(Request::ReplaceCertificate(ReplaceCertificate {
            address,
            new_certificate,
            old_fingerprint,
            new_names: vec![],
            new_expired_at: None,
        }))?;

        Ok(())
    }

    pub fn remove_certificate(
        &mut self,
        address: String,
        certificate_path: Option<&str>,
        fingerprint: Option<&str>,
    ) -> anyhow::Result<()> {
        let fingerprint = match (certificate_path, fingerprint) {
            (None, None) | (Some(_), Some(_)) => {
                bail!("Error: Please provide either one, the path OR the fingerprint of the certificate")
            }
            (Some(certificate_path), None) => {
                get_fingerprint_from_certificate_path(certificate_path).with_context(|| {
                    "Could not retrieve the finger print from the given certificate path"
                })?
            }
            (None, Some(fingerprint)) => decode_fingerprint(fingerprint)
                .with_context(|| "Error decoding the given fingerprint")?,
        };

        self.order_request(Request::RemoveCertificate(RemoveCertificate {
            address,
            fingerprint,
        }))
    }
}

fn get_fingerprint_from_certificate_path(certificate_path: &str) -> anyhow::Result<Fingerprint> {
    let bytes = Config::load_file_bytes(certificate_path)
        .with_context(|| format!("could not load certificate file on path {certificate_path}"))?;

    let parsed_bytes = calculate_fingerprint(&bytes).with_context(|| {
        format!("could not calculate fingerprint for the certificate at {certificate_path}")
    })?;

    Ok(Fingerprint(parsed_bytes))
}

fn decode_fingerprint(fingerprint: &str) -> anyhow::Result<Fingerprint> {
    let bytes = hex::decode(fingerprint)
        .with_context(|| "Failed at decoding the string (expected hexadecimal data)")?;
    Ok(Fingerprint(bytes))
}

fn load_full_certificate(
    certificate_path: &str,
    certificate_chain_path: &str,
    key_path: &str,
    versions: Vec<TlsVersion>,
) -> Result<CertificateAndKey, anyhow::Error> {
    let certificate = Config::load_file(certificate_path)
        .with_context(|| format!("Could not load certificate file on path {certificate_path}"))?;

    let certificate_chain = Config::load_file(certificate_chain_path)
        .map(split_certificate_chain)
        .with_context(|| {
            format!("could not load certificate chain on path: {certificate_chain_path}")
        })?;

    let key = Config::load_file(key_path)
        .with_context(|| format!("Could not load key file on path {key_path}"))?;

    Ok(CertificateAndKey {
        certificate,
        certificate_chain,
        key,
        versions,
    })
}
