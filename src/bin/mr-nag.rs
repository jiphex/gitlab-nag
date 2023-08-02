use anyhow::Context;
use chrono::Local;
use clap::Parser;
use gitlab::{
    api::{projects::merge_requests, AsyncQuery},
    AsyncGitlab, Gitlab, MergeRequest,
};
use reqwest::Url;
use secrecy::{ExposeSecret, SecretString};
use slack_morphism::prelude::*;
use tracing::{debug, info_span, trace};
use tracing_subscriber::{prelude::*, EnvFilter};

/// Notifier to send a Slack Webhook if open Merge Requests exist for a
/// particular Gitlab project.
///
/// This is a small one-shot utility that will check whether the specified
/// Gitlab has any open Merge Requests, with the option to filter them to a
/// specific target branch.
///
/// The general use case for this is to nag a Slack channel if there is an open
/// MR to production at a certain time during the day. To setup this behaviour,
/// the binary should be run under Cron, and given a Slack webhook URL to notify
/// whenever an MR is open.
#[derive(Debug, Parser)]
#[command(author, version)]
struct CmdArgs {
    /// Optional Webhook URL to notify if open merge requests are found
    #[arg(short, long, env)]
    slack_webhook_url: Option<Url>,
    /// Gitlab token which requires read:api access to the project in question
    #[arg(short = 't', long, env)]
    gitlab_token: SecretString,
    /// Gitlab host, e.g "gitlab.example.com". HTTPS is required, and port 443 is
    /// assumed by default.
    #[arg(short, long, env)]
    gitlab_host: String,
    /// Numeric Gitlab project ID of the project to check
    #[arg(short = 'i', long, env)]
    gitlab_project_id: u64,
    /// Optional branch to filter for - if specified, only merge requests with a
    /// target of this specific branch will trigger the notification.
    #[arg(short = 'T', long, env)]
    target_branch: Option<String>,
    /// Specify a minimum time which an MR must be "idle" for before creating a
    /// notification. Note that this may cause notifications to be missed if the
    /// execution interval is insufficient.
    #[arg(short = 'd', long, env)]
    min_dwell_secs: Option<i64>,
    /// Enable verbose logging
    #[arg(short = 'v', long, default_value_t = false)]
    verbose: bool,
}

/// Get the merge requests as per the input args, filtering for project, state (open) and target branch (if specified)
#[tracing::instrument(skip_all)]
async fn get_merge_requsts<'a>(
    args: &CmdArgs,
    gitlab: &'a AsyncGitlab,
) -> anyhow::Result<impl IntoIterator<Item = MergeRequest>> {
    let tb = args.target_branch.as_ref().map_or("", |x| &x);
    debug!(
        target_branch = tb,
        project_id = args.gitlab_project_id,
        "searching project for open MRs"
    );
    let mr_q = merge_requests::MergeRequests::builder()
        .project(args.gitlab_project_id)
        .state(merge_requests::MergeRequestState::Opened)
        .wip(false)
        .target_branch(tb)
        .build()
        .unwrap();
    // have to use let ... here to explicitly inform the type (Vec)
    let merge_requests: Vec<MergeRequest> = mr_q.query_async(gitlab).await.unwrap();
    debug!(
        mr_in_scope = merge_requests.len(),
        "merge request query complete"
    );
    Ok(merge_requests)
}

struct WrappedMR(MergeRequest);

impl SlackMessageTemplate for WrappedMR {
    fn render_template(&self) -> SlackMessageContent {
        // self.0.u
        SlackMessageContent::new()
            .with_text(format!(
                "MR #{} awaiting merge: \"{}\"",
                self.0.id, self.0.title
            ))
            .with_blocks(slack_blocks![
                some_into(SlackSectionBlock::new().with_text(md!(format!(
                    "MR **#{}: [{}]({})**",
                    self.0.id, self.0.title, self.0.web_url
                ))))
            ])
    }
}

fn setup_logging(args: &CmdArgs) -> anyhow::Result<()> {
    // First, setup LogTracer to catch events from the reqwest/gitlab crates
    let filter_layer = EnvFilter::try_from_default_env()
        .or_else(|_| match args.verbose {
            true => EnvFilter::try_new("debug"),
            false => EnvFilter::try_new("info"),
        })
        .unwrap();
    let fmtj = tracing_journald::layer().ok();
    let fmtout = if fmtj.is_none() {
        // Only setup the fmt layer if the journal connection doesn't work
        Some(tracing_subscriber::fmt::layer().with_target(false))
    } else {
        // So the journald thing worked, therefore we don't setup the fmt output, to avoid duplicate logs
        None
    };
    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmtout)
        .with(fmtj)
        .init();
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = CmdArgs::parse();
    setup_logging(&args)?;
    let gitlab = Gitlab::builder(&args.gitlab_host, args.gitlab_token.expose_secret())
        .build_async()
        .await
        .context("unable to build gitlab API client")?;
    let now = Local::now();
    for mr in get_merge_requsts(&args, &gitlab).await.unwrap() {
        let _mr_span = info_span!("processing_merge_request").entered();
        if let Some(dwell) = args.min_dwell_secs {
            // Skip this MR and continue to next if the time since update is < dwell time
            if now.signed_duration_since(mr.updated_at).num_seconds() < dwell {
                trace!(mr_id=?mr.id,updated_at=?mr.updated_at,dwell,"skipping due to insufficent dwell time");
                continue;
            }
        }
        let msg = format!(
            "MR #{} ({}) is awaiting merge{}",
            mr.id,
            mr.title,
            match &args.target_branch {
                None => ".".to_string(),
                Some(tb) => format!(" to target branch: {}.", &tb),
            }
        );
        // print the message to stdout
        println!("{msg}");
        if let Some(hook_url) = args.slack_webhook_url.clone() {
            let _slack_span = info_span!("slack_webhook_post").entered();
            let client = SlackClient::new(SlackClientHyperConnector::new());

            client
                .post_webhook_message(
                    &hook_url,
                    &SlackApiPostWebhookMessageRequest::new(WrappedMR(mr).render_template()),
                )
                .await
                .unwrap();
        }
    }
    Ok(())
}
