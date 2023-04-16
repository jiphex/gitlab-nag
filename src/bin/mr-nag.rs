use anyhow::Context;
use clap::Parser;
use gitlab::{
    api::{
        projects::{self, merge_requests},
        Query,
    },
    Gitlab, MergeRequest, Project,
};
use reqwest::Url;
use secrecy::{ExposeSecret, SecretString};
use slack_hook::{AttachmentBuilder, PayloadBuilder, Slack};

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
}

fn open_mr_list_url_for(project: &Project, target_branch: &Option<String>) -> Url {
    // We have to add a trailing slash to the URL or Url::parse will strip off
    // the last component from the URL
    let pwurl = format!("{}/", &project.web_url);
    let mut base = Url::parse(&pwurl).unwrap();
    base = base.join("-/merge_requests/").unwrap();
    // Set the URL query to filter for open ones
    let mut mr_query = String::from("?scope=all&state=opened");
    if let Some(tb) = target_branch {
        // add the filter for the specified target branch
        mr_query.push_str("&target_branch=");
        mr_query.push_str(tb);
    }
    base.set_query(Some(&mr_query));
    base
}

fn main() -> anyhow::Result<()> {
    let args = CmdArgs::parse();
    let gitlab = Gitlab::builder(args.gitlab_host, args.gitlab_token.expose_secret())
        .build()
        .context("unable to build gitlab API client")?;
    let mut mr_q_b = merge_requests::MergeRequests::builder();
    mr_q_b
        .project(args.gitlab_project_id)
        .state(merge_requests::MergeRequestState::Opened);
    if let Some(target_branch) = &args.target_branch {
        mr_q_b.target_branch(target_branch);
    }
    let mr_q = mr_q_b.build()?;
    let mrs: Vec<MergeRequest> = mr_q.query(&gitlab).unwrap();
    if let Some(first_mr) = mrs.first() {
        // We are only pulling the first here, but we only really need to notify
        // if there is at least one MR waiting, any others don't really matter.
        let target_project_query = projects::Project::builder()
            .project(first_mr.project_id.value())
            .build()?;
        let project: Project = target_project_query.query(&gitlab)?;
        let mr_list_url = open_mr_list_url_for(&project, &args.target_branch);
        let msg = format!(
            "{} MR is awaiting merge{}",
            mrs.len(),
            match &args.target_branch {
                None => ".".to_string(),
                Some(tb) => format!(" to target branch: {}.", &tb),
            }
        );
        // print the message to stdout
        println!("{msg}");
        if let Some(hook_url) = &args.slack_webhook_url {
            let hook = Slack::new(hook_url.as_str()).unwrap();
            // Create a Slack "attachment" that will give a bit more info than the text
            let msg_attachment = AttachmentBuilder::new(msg)
                .title(format!("Open Merge requests for {}", project.name))
                .title_link(mr_list_url.as_str())
                .footer("These should be merged before the end of the day if possible!")
                .build()
                .unwrap();
            // Create the message and stick the attachment on it
            let payload = PayloadBuilder::new()
                .username("Gitlab Merge Request Nagbot")
                .icon_emoji(":pencil:")
                .attachments(vec![msg_attachment])
                .build()
                .unwrap();
            // Send the message. We can just panic if it fails because we're about to exit anyway.
            hook.send(&payload).unwrap();
        }
    } else {
        // Nothing to do here
        return Ok(());
    }
    Ok(())
}
