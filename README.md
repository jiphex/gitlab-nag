Sorry, no README yet - just the `--help`.

```
Notifier to send a Slack Webhook if open Merge Requests exist for a particular Gitlab project.

This is a small one-shot utility that will check whether the specified Gitlab has any open Merge Requests, with the option to filter them to a specific target branch.

The general use case for this is to nag a Slack channel if there is an open MR to production at a certain time during the day. To setup this behaviour, the binary should be run under Cron, and given a Slack webhook URL to notify whenever an MR is open.

Usage: mr-nag [OPTIONS] --gitlab-token <GITLAB_TOKEN> --gitlab-host <GITLAB_HOST> --gitlab-project-id <GITLAB_PROJECT_ID>

Options:
  -s, --slack-webhook-url <SLACK_WEBHOOK_URL>
          Optional Webhook URL to notify if open merge requests are found

          [env: SLACK_WEBHOOK_URL=]

  -t, --gitlab-token <GITLAB_TOKEN>
          Gitlab token which requires read:api access to the project in question

          [env: GITLAB_TOKEN=]

  -g, --gitlab-host <GITLAB_HOST>
          Gitlab host, e.g "gitlab.example.com". HTTPS is required, and port 443 is assumed by default

          [env: GITLAB_HOST=]

  -i, --gitlab-project-id <GITLAB_PROJECT_ID>
          Numeric Gitlab project ID of the project to check

          [env: GITLAB_PROJECT_ID=]

  -T, --target-branch <TARGET_BRANCH>
          Optional branch to filter for - if specified, only merge requests with a target of this specific branch will trigger the notification

          [env: TARGET_BRANCH=]

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```
