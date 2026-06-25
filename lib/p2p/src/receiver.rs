use iroh::Endpoint;
use iroh_blobs::{
    HashAndFormat,
    get::{db::get_to_db, request::get_hash_seq_and_sizes},
    store::fs::Store,
};
use tokio::task::JoinHandle;

use crate::common::*;

pub async fn receive(args: ReceiveArgs) -> anyhow::Result<()> {
    let ticket = args.ticket;
    let addr = ticket.node_addr().clone();
    let secret_key = get_or_create_secret(args.common.verbose > 0)?;

    let mut builder = Endpoint::builder()
        .alpns(vec![])
        .secret_key(secret_key)
        .relay_mode(args.common.relay.into());

    // if no addresses, fallback to DNS discovery
    if ticket.node_addr().relay_url.is_none() && ticket.node_addr().direct_addresses.is_empty() {
        builder = builder.add_discovery(|_| Some(iroh::discovery::dns::DnsDiscovery::n0_dns()));
    }
    if let Some(a) = args.common.magic_ipv4_addr {
        builder = builder.bind_addr_v4(a);
    }
    if let Some(a) = args.common.magic_ipv6_addr {
        builder = builder.bind_addr_v6(a);
    }
    let endpoint = builder.bind().await?;

    let dir_name = format!(".sendme-get-{}", ticket.hash().to_hex());
    let iroh_data_dir = std::env::current_dir()?.join(dir_name);
    let db = Store::load(&iroh_data_dir).await?;

    // show a connecting spinner
    let mp = indicatif::MultiProgress::new();
    let connect_progress = mp.add(indicatif::ProgressBar::hidden());
    connect_progress.set_draw_target(indicatif::ProgressDrawTarget::stderr());
    connect_progress.set_style(indicatif::ProgressStyle::default_spinner());
    connect_progress.set_message(format!("connecting to {}", addr.node_id));

    let connection = endpoint.connect(addr, iroh_blobs::protocol::ALPN).await?;
    connect_progress.finish_and_clear();

    let hash_and_format = HashAndFormat {
        hash: ticket.hash(),
        format: ticket.format(),
    };

    let (send, recv) = async_channel::bounded(32);
    let progress = iroh_blobs::util::progress::AsyncChannelProgressSender::new(send);
    let (_hash_seq, sizes) =
        get_hash_seq_and_sizes(&connection, &hash_and_format.hash, 1024 * 1024 * 32)
            .await
            .map_err(show_get_error)?;

    let total_size = sizes.iter().sum::<u64>();
    let total_files = sizes.len().saturating_sub(1);
    let payload_size = sizes.iter().skip(1).sum::<u64>();
    eprintln!(
        "getting collection {} {} files, {}",
        print_hash(&ticket.hash(), args.common.format),
        total_files,
        indicatif::HumanBytes(payload_size)
    );
    if args.common.verbose > 0 {
        eprintln!(
            "getting {} blobs in total, {}",
            sizes.len(),
            indicatif::HumanBytes(total_size)
        );
    }

    let progress_task: JoinHandle<anyhow::Result<()>> =
        tokio::spawn(show_download_progress(recv, total_size));
    let get_conn = || async move { Ok(connection) };

    let stats = get_to_db(&db, get_conn, &hash_and_format, progress)
        .await
        .map_err(|e| show_get_error(anyhow::anyhow!(e)))?;

    let collection =
        iroh_blobs::format::collection::Collection::load_db(&db, &hash_and_format.hash).await?;
    if args.common.verbose > 0 {
        for (name, h) in collection.iter() {
            println!("    {} {name}", print_hash(&h, args.common.format));
        }
    }

    if let Some((name, _)) = collection.iter().next() {
        if let Some(first) = name.split('/').next() {
            println!("downloading to: {};", first);
        }
    }
    export(db, collection).await?;
    tokio::fs::remove_dir_all(iroh_data_dir).await?;

    progress_task.await??;

    if args.common.verbose > 0 {
        println!(
            "downloaded {} files, {}. took {} ({}/s)",
            total_files,
            indicatif::HumanBytes(payload_size),
            indicatif::HumanDuration(stats.elapsed),
            indicatif::HumanBytes((stats.bytes_read as f64 / stats.elapsed.as_secs_f64()) as u64),
        );
    }
    Ok(())
}
