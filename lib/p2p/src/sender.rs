//! Sender functionality (embedded version of `sendme send`).

use std::time::Duration;

use console::{Key, Term};
use data_encoding::HEXLOWER;
use iroh::{Endpoint, protocol::Router};
use iroh_blobs::{BlobFormat, net_protocol::Blobs, ticket::BlobTicket};
use rand::Rng;
use tokio::signal;

use crate::common::*;

pub async fn send(args: SendArgs) -> anyhow::Result<()> {
    let secret_key = get_or_create_secret(args.common.verbose > 0)?;
    // build the endpoint
    let mut builder = Endpoint::builder()
        .alpns(vec![iroh_blobs::protocol::ALPN.to_vec()])
        .secret_key(secret_key)
        .relay_mode(args.common.relay.into());

    if args.ticket_type == AddrInfoOptions::Id {
        builder = builder.add_discovery(|secret_key| {
            Some(iroh::discovery::pkarr::PkarrPublisher::n0_dns(
                secret_key.clone(),
            ))
        });
    }
    if let Some(addr) = args.common.magic_ipv4_addr {
        builder = builder.bind_addr_v4(addr);
    }
    if let Some(addr) = args.common.magic_ipv6_addr {
        builder = builder.bind_addr_v6(addr);
    }

    // store data in a random ephemeral dir so it gets cleaned after we exit
    let suffix = rand::thread_rng().gen::<[u8; 16]>();
    let cwd = std::env::current_dir()?;
    let blobs_data_dir = cwd.join(format!(".sendme-send-{}", HEXLOWER.encode(&suffix)));
    if blobs_data_dir.exists() {
        eprintln!(
            "Cannot share twice from the same directory: {}",
            cwd.display()
        );
        std::process::exit(1);
    }
    tokio::fs::create_dir_all(&blobs_data_dir).await?;

    let endpoint = builder.bind().await?;
    let ps = SendStatus::new();
    let blobs = Blobs::persistent(&blobs_data_dir)
        .await?
        .events(ps.new_client().into())
        .build(&endpoint);

    let router = Router::builder(endpoint)
        .accept(iroh_blobs::ALPN, blobs.clone())
        .spawn()
        .await?;

    let path = args.path;
    let (temp_tag, size, collection) = import(path.clone(), blobs.store().clone()).await?;
    let hash = *temp_tag.hash();

    // wait for the endpoint to figure out its address
    let _ = router.endpoint().home_relay().initialized().await?;
    let mut addr = router.endpoint().node_addr().await?;
    apply_options(&mut addr, args.ticket_type);

    let ticket = BlobTicket::new(addr, hash, BlobFormat::HashSeq)?;
    let entry_type = if path.is_file() { "file" } else { "directory" };
    println!(
        "imported {} {}, {}, hash {}",
        entry_type,
        path.display(),
        indicatif::HumanBytes(size),
        print_hash(&hash, args.common.format)
    );
    if args.common.verbose > 0 {
        for (name, h) in collection.iter() {
            println!("    {} {name}", print_hash(&h, args.common.format));
        }
    }

    println!("to get this data, use");
    println!("sendme receive {}", ticket);

    // possibly add to the clipboard
    if args.clipboard {
        add_to_clipboard(&ticket);
    }

    // spawn a background task to watch for 'c' to copy
    let clipboard_ticket = ticket.clone();
    let _keyboard = tokio::task::spawn(async move {
        let term = Term::stdout();
        println!("press c to copy command to clipboard");
        loop {
            if let Ok(Key::Char('c')) = term.read_key() {
                add_to_clipboard(&clipboard_ticket);
            }
        }
    });

    // wait for ctrl-c
    signal::ctrl_c().await?;

    drop(temp_tag);

    println!("shutting down");
    tokio::time::timeout(Duration::from_secs(2), router.shutdown()).await??;
    tokio::fs::remove_dir_all(blobs_data_dir).await?;

    Ok(())
}
