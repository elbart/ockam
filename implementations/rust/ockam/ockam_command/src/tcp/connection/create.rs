use crate::util::get_final_element;
use crate::{
    util::{api, connect_to, exitcode},
    CommandGlobalOpts,
};
use clap::Args;
use ockam::{Context, Route, TCP};
use ockam_api::{
    config::snippet::{ComposableSnippet, Operation, Protocol, RemoteMode},
    nodes::{models::transport::TransportStatus, NODEMANAGER_ADDR},
    route_to_multiaddr,
};
use ockam_core::api::Status;

#[derive(Clone, Debug, Args)]
pub struct TcpConnectionNodeOpts {
    /// Node that will initiate the connection
    #[clap(
        global = true,
        short,
        long,
        value_name = "NODE",
        default_value = "default"
    )]
    pub from: String,
}

#[derive(Args, Clone, Debug)]
pub struct CreateCommand {
    #[clap(flatten)]
    node_opts: TcpConnectionNodeOpts,

    /// The address to connect to (required)
    #[clap(name = "to", short, long, value_name = "ADDRESS")]
    pub address: String,
}

impl From<&'_ CreateCommand> for ComposableSnippet {
    fn from(cc: &'_ CreateCommand) -> Self {
        let mode = RemoteMode::Connector;
        let tcp = true;
        let address = cc.address.clone();

        Self {
            id: format!(
                "_transport_{}_{}_{}",
                mode,
                if tcp { "tcp" } else { "unknown" },
                address
            ),
            op: Operation::Transport {
                protocol: Protocol::Tcp,
                address,
                mode,
            },
            params: vec![],
        }
    }
}

impl CreateCommand {
    pub fn run(self, options: CommandGlobalOpts) {
        let cfg = &options.config;
        let node = get_final_element(&self.node_opts.from);
        let port = match cfg.select_node(node) {
            Some(cfg) => cfg.port,
            None => {
                eprintln!("No such node available.  Run `ockam node list` to list available nodes");
                std::process::exit(exitcode::IOERR);
            }
        };

        connect_to(port, self.clone(), create_connection);

        let composite = (&self).into();
        let node = node.to_string();

        let startup_config = match cfg.get_startup_cfg(&node) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("failed to load startup configuration: {}", e);
                std::process::exit(exitcode::IOERR);
            }
        };
        startup_config.add_composite(composite);
        if let Err(e) = startup_config.atomic_update().run() {
            eprintln!("failed to update configuration: {}", e);
            std::process::exit(exitcode::IOERR);
        }
    }
}

pub async fn create_connection(
    ctx: Context,
    cmd: CreateCommand,
    mut base_route: Route,
) -> anyhow::Result<()> {
    let resp: Vec<u8> = match ctx
        .send_and_receive(
            base_route.modify().append(NODEMANAGER_ADDR),
            api::create_tcp_connection(&cmd)?,
        )
        .await
    {
        Ok(sr_msg) => sr_msg,
        Err(e) => {
            eprintln!("Wasn't able to send or receive `Message`: {}", e);
            std::process::exit(exitcode::IOERR);
        }
    };

    let (response, TransportStatus { payload, .. }) = api::parse_transport_status(&resp)?;

    match response.status() {
        Some(Status::Ok) => {
            let r: Route = base_route
                .modify()
                .pop_back()
                .append_t(TCP, payload.to_string())
                .into();
            let multiaddr = match route_to_multiaddr(&r) {
                Some(addr) => addr,
                None => {
                    eprintln!("Couldn't convert given address into `MultiAddr`");
                    std::process::exit(exitcode::SOFTWARE);
                }
            };

            println!(
                "Tcp connection created! You can send messages to it via this route:\n`{}`",
                multiaddr
            )
        }
        _ => {
            eprintln!(
                "An error occurred while creating the tcp connection: {}",
                payload
            );
            std::process::exit(exitcode::CANTCREAT);
        }
    }
    Ok(())
}
