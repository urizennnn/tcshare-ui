use std::{
    collections::BTreeMap,
    fmt::{Display, Formatter},
    net::{SocketAddrV4, SocketAddrV6},
    path::{Component, Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use arboard::Clipboard;
use console::style;
use futures_buffered::BufferedStreamExt;
use indicatif::{
    HumanBytes, HumanDuration, MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle,
};
use iroh::{
    NodeAddr, RelayMap, RelayMode, RelayUrl, SecretKey,
};
use iroh_blobs::{
    BlobFormat, Hash, TempTag,
    format::collection::Collection,
    get::{
        db::DownloadProgress,
        fsm::{AtBlobHeaderNextError, DecodeError},
    },
    provider::{self, CustomEventSender},
    store::{ExportMode, ImportMode, ImportProgress},
    ticket::BlobTicket,
};
use n0_future::{StreamExt, future::Boxed};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use walkdir::WalkDir;

///
/// # Format
///
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    #[default]
    Hex,
    Cid,
}

impl FromStr for Format {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "hex" => Ok(Format::Hex),
            "cid" => Ok(Format::Cid),
            _ => Err(anyhow::anyhow!("invalid format")),
        }
    }
}

impl Display for Format {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Format::Hex => write!(f, "hex"),
            Format::Cid => write!(f, "cid"),
        }
    }
}

pub fn print_hash(hash: &Hash, format: Format) -> String {
    match format {
        Format::Hex => hash.to_hex().to_string(),
        Format::Cid => hash.to_string(),
    }
}

///
/// # Commands
///
/// Note: This enum was originally for CLI subcommands. We retain it to avoid
/// omitting code, but you don't have to use it in your TUI.
///
#[derive(clap::Subcommand, Debug)]
pub enum Commands {
    /// Send a file or directory.
    Send(SendArgs),

    /// Receive a file or directory.
    Receive(ReceiveArgs),
}

///
/// # CommonArgs
///
/// Fields that were originally shared among subcommands.
///
#[derive(clap::Parser, Debug)]
pub struct CommonArgs {
    /// The IPv4 address that magicsocket will listen on.
    #[clap(long, default_value = None)]
    pub magic_ipv4_addr: Option<SocketAddrV4>,

    /// The IPv6 address that magicsocket will listen on.
    #[clap(long, default_value = None)]
    pub magic_ipv6_addr: Option<SocketAddrV6>,

    #[clap(long, default_value_t = Format::Hex)]
    pub format: Format,

    #[clap(short = 'v', long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// The relay URL to use as a home relay,
    #[clap(long, default_value_t = RelayModeOption::Default)]
    pub relay: RelayModeOption,
}

///
/// # RelayModeOption
///
#[derive(Clone, Debug)]
pub enum RelayModeOption {
    Disabled,
    Default,
    Custom(RelayUrl),
}

impl FromStr for RelayModeOption {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "disabled" => Ok(Self::Disabled),
            "default" => Ok(Self::Default),
            _ => Ok(Self::Custom(RelayUrl::from_str(s)?)),
        }
    }
}

impl Display for RelayModeOption {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => f.write_str("disabled"),
            Self::Default => f.write_str("default"),
            Self::Custom(url) => url.fmt(f),
        }
    }
}

impl From<RelayModeOption> for RelayMode {
    fn from(value: RelayModeOption) -> Self {
        match value {
            RelayModeOption::Disabled => RelayMode::Disabled,
            RelayModeOption::Default => RelayMode::Default,
            RelayModeOption::Custom(url) => RelayMode::Custom(RelayMap::from_url(url)),
        }
    }
}

///
/// # SendArgs
///
#[derive(clap::Parser, Debug)]
pub struct SendArgs {
    /// Path to the file or directory to send.
    pub path: PathBuf,

    /// What type of ticket to use. (id, addresses, relay, or relay+addresses).
    #[clap(long, default_value_t = AddrInfoOptions::RelayAndAddresses)]
    pub ticket_type: AddrInfoOptions,

    #[clap(flatten)]
    pub common: CommonArgs,

    /// Store the receive command in the clipboard.
    #[clap(short = 'c', long)]
    pub clipboard: bool,
}

///
/// # ReceiveArgs
///
#[derive(clap::Parser, Debug)]
pub struct ReceiveArgs {
    /// The ticket to use to connect to the sender.
    pub ticket: BlobTicket,

    #[clap(flatten)]
    pub common: CommonArgs,
}

///
/// # AddrInfoOptions
///
#[derive(
    Copy,
    Clone,
    PartialEq,
    Eq,
    Default,
    Debug,
    derive_more::Display,
    derive_more::FromStr,
    Serialize,
    Deserialize,
)]
pub enum AddrInfoOptions {
    /// Only the Node ID is added.
    #[default]
    Id,
    /// Includes Node ID and both the relay URL and direct addresses.
    RelayAndAddresses,
    /// Includes the Node ID and the relay URL.
    Relay,
    /// Includes the Node ID and the direct addresses.
    Addresses,
}

/// Applies the user-chosen [`AddrInfoOptions`] to the given [`NodeAddr`].
pub fn apply_options(addr: &mut NodeAddr, opts: AddrInfoOptions) {
    match opts {
        AddrInfoOptions::Id => {
            addr.direct_addresses.clear();
            addr.relay_url = None;
        }
        AddrInfoOptions::RelayAndAddresses => {
            // do nothing
        }
        AddrInfoOptions::Relay => {
            addr.direct_addresses.clear();
        }
        AddrInfoOptions::Addresses => {
            addr.relay_url = None;
        }
    }
}

///
/// # get_or_create_secret
///
/// Get the secret key from IROH_SECRET or generate a new one.
///
pub fn get_or_create_secret(print: bool) -> anyhow::Result<SecretKey> {
    match std::env::var("IROH_SECRET") {
        Ok(secret) => SecretKey::from_str(&secret).context("invalid secret"),
        Err(_) => {
            let key = SecretKey::generate(rand::rngs::OsRng);
            if print {
                eprintln!("using secret key {}", key);
            }
            Ok(key)
        }
    }
}

/// Check that a path component doesn't contain forbidden separators.
pub fn validate_path_component(component: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !component.contains('/'),
        "path components must not contain the path separator /"
    );
    Ok(())
}

/// Convert an already-canonicalized path to a string, ensuring it’s relative
/// and has no invalid components.
pub fn canonicalized_path_to_string(
    path: impl AsRef<Path>,
    must_be_relative: bool,
) -> anyhow::Result<String> {
    let mut path_str = String::new();
    let parts = path
        .as_ref()
        .components()
        .filter_map(|c| match c {
            Component::Normal(x) => {
                let c = match x.to_str() {
                    Some(c) => c,
                    None => return Some(Err(anyhow::anyhow!("invalid character in path"))),
                };
                if !c.contains('/') && !c.contains('\\') {
                    Some(Ok(c))
                } else {
                    Some(Err(anyhow::anyhow!("invalid path component {:?}", c)))
                }
            }
            Component::RootDir => {
                if must_be_relative {
                    Some(Err(anyhow::anyhow!("invalid path component {:?}", c)))
                } else {
                    path_str.push('/');
                    None
                }
            }
            _ => Some(Err(anyhow::anyhow!("invalid path component {:?}", c))),
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let parts = parts.join("/");
    path_str.push_str(&parts);
    Ok(path_str)
}

/// Progress display for file ingestion.
pub async fn show_ingest_progress(
    recv: async_channel::Receiver<ImportProgress>,
) -> anyhow::Result<()> {
    let mp = MultiProgress::new();
    mp.set_draw_target(ProgressDrawTarget::stderr());
    let op = mp.add(ProgressBar::hidden());
    op.set_style(
        ProgressStyle::default_spinner().template("{spinner:.green} [{elapsed_precise}] {msg}")?,
    );

    let mut names = BTreeMap::new();
    let mut sizes = BTreeMap::new();
    let mut pbs = BTreeMap::new();
    loop {
        let event = recv.recv().await;
        match event {
            Ok(ImportProgress::Found { id, name }) => {
                names.insert(id, name);
            }
            Ok(ImportProgress::Size { id, size }) => {
                sizes.insert(id, size);
                let total_size = sizes.values().sum::<u64>();
                op.set_message(format!(
                    "[1/2] Ingesting {} files, {}\n",
                    sizes.len(),
                    HumanBytes(total_size)
                ));
                let name = names.get(&id).cloned().unwrap_or_default();
                let pb = mp.add(ProgressBar::hidden());
                pb.set_style(
                    ProgressStyle::with_template(
                        "{msg}{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes}",
                    )?
                    .progress_chars("#>-"),
                );
                pb.set_message(format!("[2/2] {name}"));
                pb.set_length(size);
                pbs.insert(id, pb);
            }
            Ok(ImportProgress::OutboardProgress { id, offset }) => {
                if let Some(pb) = pbs.get(&id) {
                    pb.set_position(offset);
                }
            }
            Ok(ImportProgress::OutboardDone { id, .. }) => {
                if let Some(pb) = pbs.remove(&id) {
                    pb.finish_and_clear();
                }
            }
            Ok(ImportProgress::CopyProgress { .. }) => {
                // for directory references
            }
            Err(e) => {
                op.set_message(format!("Error receiving progress: {e}"));
                break;
            }
        }
    }
    op.finish_and_clear();
    Ok(())
}

/// Import from a file or directory into the database. Returns a `(TempTag, total_size, Collection)`.
pub async fn import(
    path: PathBuf,
    db: impl iroh_blobs::store::Store,
) -> anyhow::Result<(TempTag, u64, Collection)> {
    let path = path.canonicalize()?;
    anyhow::ensure!(path.exists(), "path {} does not exist", path.display());
    let root = path.parent().context("get parent")?;
    let files = WalkDir::new(path.clone()).into_iter();

    // flatten the directory structure into a list of (name, path) pairs
    let data_sources: Vec<(String, PathBuf)> = files
        .map(|entry| {
            let entry = entry?;
            if !entry.file_type().is_file() {
                return Ok(None);
            }
            let p = entry.into_path();
            let relative = p.strip_prefix(root)?;
            let name = canonicalized_path_to_string(relative, true)?;
            Ok(Some((name, p)))
        })
        .filter_map(Result::transpose)
        .collect::<anyhow::Result<Vec<_>>>()?;

    let (send, recv) = async_channel::bounded(32);
    let progress = iroh_blobs::util::progress::AsyncChannelProgressSender::new(send);
    let show_progress: JoinHandle<anyhow::Result<()>> = tokio::spawn(show_ingest_progress(recv));

    let mut names_and_tags = futures_lite::stream::iter(data_sources)
        .map(|(name, path)| {
            let db = db.clone();
            let progress = progress.clone();
            async move {
                let (temp_tag, file_size) = db
                    .import_file(path, ImportMode::TryReference, BlobFormat::Raw, progress)
                    .await?;
                Ok((name, temp_tag, file_size))
            }
        })
        .buffered_unordered(num_cpus::get())
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<anyhow::Result<Vec<_>>>()?;

    drop(progress);
    names_and_tags.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));
    let size = names_and_tags.iter().map(|(_, _, sz)| *sz).sum::<u64>();
    let (collection, tags): (Collection, Vec<TempTag>) = names_and_tags
        .into_iter()
        .map(|(n, t, _)| ((n, *t.hash()), t))
        .unzip();

    let temp_tag = collection.clone().store(&db).await?;
    drop(tags);
    show_progress.await??;

    Ok((temp_tag, size, collection))
}

/// Return a path on disk under `root` for the given `name`.
pub fn get_export_path(root: &Path, name: &str) -> anyhow::Result<PathBuf> {
    let parts = name.split('/');
    let mut path = root.to_path_buf();
    for part in parts {
        validate_path_component(part)?;
        path.push(part);
    }
    Ok(path)
}

/// Export a collection from the DB to disk in the current working directory.
pub async fn export(
    db: impl iroh_blobs::store::Store,
    collection: Collection,
) -> anyhow::Result<()> {
    let root = std::env::current_dir()?;
    for (name, hash) in collection.iter() {
        let target = get_export_path(&root, name)?;
        if target.exists() {
            eprintln!(
                "target {} already exists. Export stopped.",
                target.display()
            );
            eprintln!("You can remove the file or directory and try again.");
            anyhow::bail!("target {} already exists", target.display());
        }
        db.export(
            *hash,
            target,
            ExportMode::TryReference,
            Box::new(|_| Ok(())),
        )
        .await?;
    }
    Ok(())
}

/// Wrapper for tracking upload (provider) status.
#[derive(Debug, Clone)]
pub struct SendStatus {
    pub mp: MultiProgress,
}

impl SendStatus {
    pub fn new() -> Self {
        let mp = MultiProgress::new();
        mp.set_draw_target(ProgressDrawTarget::stderr());
        Self { mp }
    }

    pub fn new_client(&self) -> ClientStatus {
        let current = self.mp.add(ProgressBar::hidden());
        current.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} [{elapsed_precise}] {msg}")
                .unwrap(),
        );
        current.enable_steady_tick(Duration::from_millis(100));
        current.set_message("waiting for requests");
        ClientStatus {
            current: Arc::new(current),
        }
    }
}

/// Provider event logging to a progress bar.
#[derive(Debug, Clone)]
pub struct ClientStatus {
    pub current: Arc<ProgressBar>,
}

impl Drop for ClientStatus {
    fn drop(&mut self) {
        // if this is the only reference, finish it
        if Arc::strong_count(&self.current) == 1 {
            self.current.finish_and_clear();
        }
    }
}

impl CustomEventSender for ClientStatus {
    fn send(&self, event: provider::Event) -> Boxed<()> {
        self.try_send(event);
        Box::pin(std::future::ready(()))
    }

    fn try_send(&self, event: provider::Event) {
        let msg = match event {
            provider::Event::ClientConnected { connection_id } => {
                Some(format!("{} got connection", connection_id))
            }
            provider::Event::TransferBlobCompleted {
                connection_id,
                hash,
                index,
                size,
                ..
            } => Some(format!(
                "{} transfer blob completed {} {} {}",
                connection_id,
                hash,
                index,
                HumanBytes(size)
            )),
            provider::Event::TransferCompleted {
                connection_id,
                stats,
                ..
            } => Some(format!(
                "{} transfer completed {} {}",
                connection_id,
                stats.send.write_bytes.size,
                HumanDuration(stats.send.write_bytes.stats.duration)
            )),
            provider::Event::TransferAborted { connection_id, .. } => {
                Some(format!("{} transfer aborted", connection_id))
            }
            _ => None,
        };
        if let Some(m) = msg {
            self.current.set_message(m);
        }
    }
}

/// Copy the "receive" command to clipboard if desired.
pub fn add_to_clipboard(ticket: &BlobTicket) {
    let clipboard = Clipboard::new();
    match clipboard {
        Ok(mut clip) => {
            if let Err(e) = clip.set_text(format!("sendme receive {}", ticket)) {
                eprintln!("Could not add to clipboard: {}", e);
            } else {
                println!("Command added to clipboard.")
            }
        }
        Err(e) => eprintln!("Could not access clipboard: {}", e),
    }
}

/// Creates a progress bar for downloads.
pub fn make_download_progress() -> ProgressBar {
    let pb = ProgressBar::hidden();
    pb.enable_steady_tick(Duration::from_millis(100));
    pb.set_style(
        ProgressStyle::with_template(
            "{msg}{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} {binary_bytes_per_sec}",
        )
        .unwrap()
        .progress_chars("#>-"),
    );
    pb
}

/// Show the streaming download progress for all blobs.
pub async fn show_download_progress(
    recv: async_channel::Receiver<DownloadProgress>,
    total_size: u64,
) -> anyhow::Result<()> {
    let mp = MultiProgress::new();
    mp.set_draw_target(ProgressDrawTarget::stderr());
    let op = mp.add(make_download_progress());
    op.set_message(format!("{} Connecting ...\n", style("[1/3]").bold().dim()));
    let mut total_done = 0;
    let mut sizes = BTreeMap::new();
    loop {
        let x = recv.recv().await;
        match x {
            Ok(DownloadProgress::Connected) => {
                op.set_message(format!("{} Requesting ...\n", style("[2/3]").bold().dim()));
            }
            Ok(DownloadProgress::FoundHashSeq { children, .. }) => {
                op.set_message(format!(
                    "{} Downloading {} blob(s)\n",
                    style("[3/3]").bold().dim(),
                    children + 1,
                ));
                op.set_length(total_size);
                op.reset();
            }
            Ok(DownloadProgress::Found { id, size, .. }) => {
                sizes.insert(id, size);
            }
            Ok(DownloadProgress::Progress { offset, .. }) => {
                op.set_position(total_done + offset);
            }
            Ok(DownloadProgress::Done { id }) => {
                total_done += sizes.remove(&id).unwrap_or_default();
            }
            Ok(DownloadProgress::AllDone(stats)) => {
                op.finish_and_clear();
                eprintln!(
                    "Transferred {} in {}, {}/s",
                    HumanBytes(stats.bytes_read),
                    HumanDuration(stats.elapsed),
                    HumanBytes((stats.bytes_read as f64 / stats.elapsed.as_secs_f64()) as u64)
                );
                break;
            }
            Ok(DownloadProgress::Abort(e)) => {
                anyhow::bail!("download aborted: {e:?}");
            }
            Err(e) => {
                anyhow::bail!("error reading progress: {e:?}");
            }
            _ => {}
        }
    }
    Ok(())
}

/// Print more specific user-friendly errors for partial fetch or mismatch.
pub fn show_get_error(e: anyhow::Error) -> anyhow::Error {
    if let Some(err) = e.downcast_ref::<DecodeError>() {
        match err {
            DecodeError::NotFound => {
                eprintln!("{}", style("send side no longer has a file").yellow())
            }
            DecodeError::LeafNotFound(_) | DecodeError::ParentNotFound(_) => eprintln!(
                "{}",
                style("send side no longer has part of a file").yellow()
            ),
            DecodeError::Io(err) => eprintln!(
                "{}",
                style(format!("generic network error: {}", err)).yellow()
            ),
            DecodeError::Read(err) => eprintln!(
                "{}",
                style(format!("error reading data from quinn: {}", err)).yellow()
            ),
            DecodeError::LeafHashMismatch(_) | DecodeError::ParentHashMismatch(_) => {
                eprintln!("{}", style("send side sent wrong data").red())
            }
        };
    } else if let Some(header_error) = e.downcast_ref::<AtBlobHeaderNextError>() {
        match header_error {
            AtBlobHeaderNextError::Io(err) => eprintln!(
                "{}",
                style(format!("generic network error: {}", err)).yellow()
            ),
            AtBlobHeaderNextError::Read(err) => eprintln!(
                "{}",
                style(format!("error reading data from quinn: {}", err)).yellow()
            ),
            AtBlobHeaderNextError::NotFound => {
                eprintln!("{}", style("send side no longer has a file").yellow())
            }
        };
    } else {
        eprintln!(
            "{}",
            style(format!("generic error: {:?}", e.root_cause())).red()
        );
    }
    e
}
