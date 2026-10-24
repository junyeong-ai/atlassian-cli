# CLI reference

Generated from the command definitions. Run `cargo run --locked --example cli_reference > docs/cli.md` to refresh.

Root options go before the subcommand. Run `atlassian-cli <command> --help` for local help.

## `atlassian-cli`

```text
CLI for Atlassian Jira and Confluence

Usage: atlassian-cli [OPTIONS] <COMMAND>

Commands:
  jira         Issues, comments, links, worklogs, watchers, and agile boards
  confluence   Pages, comment threads, labels, properties, spaces, and attachments
  config       Create, inspect, or validate the configuration this tool reads
  auth         Sign in, sign out, and report or refresh the stored session
  self         Inspect, update, or remove this installation
  completions  Generate a shell completion script on stdout
  help         Print this message or the help of the given subcommand(s)

Options:
      --config <CONFIG>
          Config file path

      --profile <PROFILE>
          Profile name

      --domain <DOMAIN>
          [env: ATLASSIAN_DOMAIN]

      --email <EMAIL>
          [env: ATLASSIAN_EMAIL]

      --token <TOKEN>
          [env: ATLASSIAN_API_TOKEN]

      --client-id <CLIENT_ID>
          [env: ATLASSIAN_CLIENT_ID]

      --client-secret <CLIENT_SECRET>
          [env: ATLASSIAN_CLIENT_SECRET]

      --cloud-id <CLOUD_ID>
          [env: ATLASSIAN_CLOUD_ID]

      --pretty
          Pretty-print JSON output

  -v, --verbose...
          Verbose logging

  -h, --help
          Print help

  -V, --version
          Print version
```

## `atlassian-cli jira`

```text
Issues, comments, links, worklogs, watchers, and agile boards

Usage: atlassian-cli jira <COMMAND>

Commands:
  get         Fetch a single issue by key (fields filtered, ADF rendered)
  search      Search issues with JQL; the configured project filter is auto-injected
  create      Create an issue
  update      Update an issue's fields from a JSON object (e.g. '{"summary":"..."}')
  delete      Permanently delete an issue (irreversible — requires --yes)
  comment     Add, update, or list comments on an issue
  transition  Apply or list workflow transitions for an issue
  link        Create, remove, or list issue links (and list link types)
  worklog     Add, update, list, or remove worklog (time-tracking) entries
  watcher     Start watching, stop watching, or list watchers on an issue
  user        Find users and the account ids that assignee fields, JQL and CQL take
  list        Query global metadata (issue types, priorities, statuses, labels)
  board       List agile boards for a project
  sprint      List sprints, or move issues between a sprint and the backlog
  epic        Assign issues to an epic, or remove them from their epics
  help        Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira get`

```text
Fetch a single issue by key (fields filtered, ADF rendered)

Usage: atlassian-cli jira get [OPTIONS] <ISSUE_KEY>

Arguments:
  <ISSUE_KEY>


Options:
      --fields <FIELDS>
          Fields to return (default: essential set + configured custom fields; use *all for the full issue)

      --format <FORMAT>
          Format of rich-text fields in the output

          Possible values:
          - adf:      Atlassian Document Format JSON, as Jira stores it; writes also take plain text
          - markdown: Markdown, converted to and from ADF

          [default: adf]

  -h, --help
          Print help (see a summary with '-h')
```

## `atlassian-cli jira search`

```text
Search issues with JQL; the configured project filter is auto-injected

Usage: atlassian-cli jira search [OPTIONS] <JQL>

Arguments:
  <JQL>


Options:
      --limit <LIMIT>
          Results per page without --all; --all requests 100 per page

          [default: 100]

      --all
          Fetch all results via token pagination

      --stream
          Stream as JSONL (requires --all)

      --fields <FIELDS>
          Fields to return

      --format <FORMAT>
          Format of rich-text fields in the output; markdown also returns the description

          Possible values:
          - adf:      Atlassian Document Format JSON, as Jira stores it; writes also take plain text
          - markdown: Markdown, converted to and from ADF

          [default: adf]

  -h, --help
          Print help (see a summary with '-h')
```

## `atlassian-cli jira create`

```text
Create an issue

Usage: atlassian-cli jira create [OPTIONS] <PROJECT> <SUMMARY> <ISSUE_TYPE>

Arguments:
  <PROJECT>


  <SUMMARY>


  <ISSUE_TYPE>


Options:
      --description <DESCRIPTION>
          Issue description; `-` reads it from standard input

      --parent <PARENT>
          Parent issue key — required by every sub-task type, and how an issue goes under its epic

      --fields <FIELDS>
          Any other field the create screen requires, as a JSON object — '{"components":[{"name":"api"}],"customfield_10010":"X"}'

      --format <FORMAT>
          Format of the description, given with --description or in --fields

          Possible values:
          - adf:      Atlassian Document Format JSON, as Jira stores it; writes also take plain text
          - markdown: Markdown, converted to and from ADF

          [default: adf]

  -h, --help
          Print help (see a summary with '-h')
```

## `atlassian-cli jira update`

```text
Update an issue's fields from a JSON object (e.g. '{"summary":"..."}')

Usage: atlassian-cli jira update [OPTIONS] <ISSUE_KEY> <FIELDS>

Arguments:
  <ISSUE_KEY>


  <FIELDS>
          Field values as a JSON object; `-` reads it from standard input

Options:
      --format <FORMAT>
          Format of a string `description` among the fields

          Possible values:
          - adf:      Atlassian Document Format JSON, as Jira stores it; writes also take plain text
          - markdown: Markdown, converted to and from ADF

          [default: adf]

  -h, --help
          Print help (see a summary with '-h')
```

## `atlassian-cli jira delete`

```text
Permanently delete an issue (irreversible — requires --yes)

Usage: atlassian-cli jira delete [OPTIONS] <ISSUE_KEY>

Arguments:
  <ISSUE_KEY>


Options:
      --yes
          Confirm the irreversible deletion

      --delete-subtasks
          Also delete subtasks (Jira rejects the call otherwise when present)

  -h, --help
          Print help
```

## `atlassian-cli jira comment`

```text
Add, update, or list comments on an issue

Usage: atlassian-cli jira comment <COMMAND>

Commands:
  add     Add a comment to an issue
  update  Update an existing comment
  list    List comments on an issue
  delete  Delete a comment by id
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira comment add`

```text
Add a comment to an issue

Usage: atlassian-cli jira comment add [OPTIONS] <ISSUE_KEY> <TEXT>

Arguments:
  <ISSUE_KEY>


  <TEXT>
          Comment text; `-` reads it from standard input

Options:
      --format <FORMAT>
          Format of the comment text

          Possible values:
          - adf:      Atlassian Document Format JSON, as Jira stores it; writes also take plain text
          - markdown: Markdown, converted to and from ADF

          [default: adf]

  -h, --help
          Print help (see a summary with '-h')
```

## `atlassian-cli jira comment update`

```text
Update an existing comment

Usage: atlassian-cli jira comment update [OPTIONS] <ISSUE_KEY> <COMMENT_ID> <TEXT>

Arguments:
  <ISSUE_KEY>


  <COMMENT_ID>


  <TEXT>
          Comment text; `-` reads it from standard input

Options:
      --format <FORMAT>
          Format of the comment text

          Possible values:
          - adf:      Atlassian Document Format JSON, as Jira stores it; writes also take plain text
          - markdown: Markdown, converted to and from ADF

          [default: adf]

  -h, --help
          Print help (see a summary with '-h')
```

## `atlassian-cli jira comment list`

```text
List comments on an issue

Usage: atlassian-cli jira comment list [OPTIONS] <ISSUE_KEY>

Arguments:
  <ISSUE_KEY>


Options:
      --format <FORMAT>
          Format of comment bodies in the output

          Possible values:
          - adf:      Atlassian Document Format JSON, as Jira stores it; writes also take plain text
          - markdown: Markdown, converted to and from ADF

          [default: adf]

  -h, --help
          Print help (see a summary with '-h')
```

## `atlassian-cli jira comment delete`

```text
Delete a comment by id

Usage: atlassian-cli jira comment delete <ISSUE_KEY> <COMMENT_ID>

Arguments:
  <ISSUE_KEY>


  <COMMENT_ID>


Options:
  -h, --help
          Print help
```

## `atlassian-cli jira transition`

```text
Apply or list workflow transitions for an issue

Usage: atlassian-cli jira transition <COMMAND>

Commands:
  apply  Apply a transition to an issue
  list   List available transitions for an issue
  help   Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira transition apply`

```text
Apply a transition to an issue

Usage: atlassian-cli jira transition apply <ISSUE_KEY> <TRANSITION_ID>

Arguments:
  <ISSUE_KEY>


  <TRANSITION_ID>


Options:
  -h, --help
          Print help
```

## `atlassian-cli jira transition list`

```text
List available transitions for an issue

Usage: atlassian-cli jira transition list <ISSUE_KEY>

Arguments:
  <ISSUE_KEY>


Options:
  -h, --help
          Print help
```

## `atlassian-cli jira link`

```text
Create, remove, or list issue links (and list link types)

Usage: atlassian-cli jira link <COMMAND>

Commands:
  types   List available link types
  add     Create a link between two issues
  remove  Remove a link — by the two issues it runs between, or by its own id
  list    List links on an issue
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira link types`

```text
List available link types

Usage: atlassian-cli jira link types

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira link add`

```text
Create a link between two issues

Usage: atlassian-cli jira link add [OPTIONS] <SOURCE> <TARGET>

Arguments:
  <SOURCE>
          Source issue key (outward side: "A blocks B" → A)

  <TARGET>
          Target issue key (inward side: "A blocks B" → B)

Options:
      --type <LINK_TYPE>
          Link type name

          [default: Relates]

      --comment <COMMENT>
          Comment to add with the link

  -h, --help
          Print help
```

## `atlassian-cli jira link remove`

```text
Remove a link — by the two issues it runs between, or by its own id

Usage: atlassian-cli jira link remove [OPTIONS] [SOURCE] [TARGET]

Arguments:
  [SOURCE]
          Source issue key

  [TARGET]
          Target issue key

Options:
      --type <LINK_TYPE>
          Link type (required when multiple link types exist between the pair)

      --id <ID>
          Remove the link with this id, as `jira link list` reports it

  -h, --help
          Print help
```

## `atlassian-cli jira link list`

```text
List links on an issue

Usage: atlassian-cli jira link list <ISSUE_KEY>

Arguments:
  <ISSUE_KEY>
          Issue key

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira worklog`

```text
Add, update, list, or remove worklog (time-tracking) entries

Usage: atlassian-cli jira worklog <COMMAND>

Commands:
  add     Add a worklog entry to an issue
  list    List worklog entries on an issue
  update  Update a worklog entry
  remove  Remove a worklog entry
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira worklog add`

```text
Add a worklog entry to an issue

Usage: atlassian-cli jira worklog add [OPTIONS] <ISSUE_KEY> <TIME_SPENT>

Arguments:
  <ISSUE_KEY>
          Issue key

  <TIME_SPENT>
          Time spent (e.g., "2h 30m", "1d", "45m")

Options:
      --comment <COMMENT>
          Comment describing the work

      --started <STARTED>
          When the work started, in the shape Jira takes it — 2026-08-22T09:30:00.000+0900, with the milliseconds and an offset carrying no colon. Omitted, Jira records now

  -h, --help
          Print help
```

## `atlassian-cli jira worklog list`

```text
List worklog entries on an issue

Usage: atlassian-cli jira worklog list <ISSUE_KEY>

Arguments:
  <ISSUE_KEY>
          Issue key

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira worklog update`

```text
Update a worklog entry

Usage: atlassian-cli jira worklog update [OPTIONS] <ISSUE_KEY> <WORKLOG_ID> <TIME_SPENT>

Arguments:
  <ISSUE_KEY>
          Issue key

  <WORKLOG_ID>
          Worklog ID

  <TIME_SPENT>
          New time spent

Options:
      --comment <COMMENT>
          Updated comment

  -h, --help
          Print help
```

## `atlassian-cli jira worklog remove`

```text
Remove a worklog entry

Usage: atlassian-cli jira worklog remove <ISSUE_KEY> <WORKLOG_ID>

Arguments:
  <ISSUE_KEY>
          Issue key

  <WORKLOG_ID>
          Worklog ID

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira watcher`

```text
Start watching, stop watching, or list watchers on an issue

Usage: atlassian-cli jira watcher <COMMAND>

Commands:
  add     Start watching an issue (adds current user)
  remove  Stop watching an issue (removes current user)
  list    List watchers on an issue
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira watcher add`

```text
Start watching an issue (adds current user)

Usage: atlassian-cli jira watcher add <ISSUE_KEY>

Arguments:
  <ISSUE_KEY>
          Issue key

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira watcher remove`

```text
Stop watching an issue (removes current user)

Usage: atlassian-cli jira watcher remove <ISSUE_KEY>

Arguments:
  <ISSUE_KEY>
          Issue key

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira watcher list`

```text
List watchers on an issue

Usage: atlassian-cli jira watcher list <ISSUE_KEY>

Arguments:
  <ISSUE_KEY>
          Issue key

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira user`

```text
Find users and the account ids that assignee fields, JQL and CQL take

Usage: atlassian-cli jira user <COMMAND>

Commands:
  search  Search users whose display name or email matches the query
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira user search`

```text
Search users whose display name or email matches the query

Usage: atlassian-cli jira user search [OPTIONS] <QUERY>

Arguments:
  <QUERY>


Options:
      --limit <LIMIT>
          Maximum number of users to return

          [default: 50]

  -h, --help
          Print help
```

## `atlassian-cli jira list`

```text
Query global metadata (issue types, priorities, statuses, labels)

Usage: atlassian-cli jira list <COMMAND>

Commands:
  types       List available issue types
  priorities  List available priorities
  statuses    List available statuses
  labels      List available labels
  help        Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira list types`

```text
List available issue types

Usage: atlassian-cli jira list types

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira list priorities`

```text
List available priorities

Usage: atlassian-cli jira list priorities

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira list statuses`

```text
List available statuses

Usage: atlassian-cli jira list statuses

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira list labels`

```text
List available labels

Usage: atlassian-cli jira list labels

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira board`

```text
List agile boards for a project

Usage: atlassian-cli jira board <COMMAND>

Commands:
  list  List boards for a project
  help  Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira board list`

```text
List boards for a project

Usage: atlassian-cli jira board list --project <PROJECT>

Options:
      --project <PROJECT>
          Project key or ID

  -h, --help
          Print help
```

## `atlassian-cli jira sprint`

```text
List sprints, or move issues between a sprint and the backlog

Usage: atlassian-cli jira sprint <COMMAND>

Commands:
  list     List sprints on a board
  move     Move issues to a sprint
  backlog  Move issues to the backlog
  help     Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira sprint list`

```text
List sprints on a board

Usage: atlassian-cli jira sprint list [OPTIONS]

Options:
      --board <BOARD>
          Board ID

      --project <PROJECT>
          Project key (auto-resolves board)

      --state <STATE>
          Sprint state filter

          [default: active,future]

  -h, --help
          Print help
```

## `atlassian-cli jira sprint move`

```text
Move issues to a sprint

Usage: atlassian-cli jira sprint move <SPRINT_ID> <ISSUES>...

Arguments:
  <SPRINT_ID>
          Sprint ID

  <ISSUES>...
          Issue keys to move

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira sprint backlog`

```text
Move issues to the backlog

Usage: atlassian-cli jira sprint backlog <ISSUES>...

Arguments:
  <ISSUES>...
          Issue keys to move

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira epic`

```text
Assign issues to an epic, or remove them from their epics

Usage: atlassian-cli jira epic <COMMAND>

Commands:
  assign    Put issues under an epic by setting their parent
  unassign  Take issues out of their epic by clearing their parent
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira epic assign`

```text
Put issues under an epic by setting their parent

Usage: atlassian-cli jira epic assign <EPIC_KEY> <ISSUES>...

Arguments:
  <EPIC_KEY>
          Epic issue key

  <ISSUES>...
          Issue keys to assign

Options:
  -h, --help
          Print help
```

## `atlassian-cli jira epic unassign`

```text
Take issues out of their epic by clearing their parent

Usage: atlassian-cli jira epic unassign <ISSUES>...

Arguments:
  <ISSUES>...
          Issue keys to unassign

Options:
  -h, --help
          Print help
```

## `atlassian-cli confluence`

```text
Pages, comment threads, labels, properties, spaces, and attachments

Usage: atlassian-cli confluence <COMMAND>

Commands:
  search      Search pages with CQL; the configured space filter is auto-injected
  get         Fetch a single page by ID
  create      Create a page
  update      Replace a page's title and body
  children    List a page's direct children: pages, folders, whiteboards, databases and embeds
  comment     Read a page's comments and threads, or write footer comments
  label       List, add, or remove labels on a page
  property    List, set, or delete content properties (structured JSON metadata) on a page
  space       List spaces, or fetch a single space by key
  attachment  List attachments on a page, or upload a file
  delete      Move a page to the trash (recoverable — requires --yes)
  help        Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli confluence search`

```text
Search pages with CQL; the configured space filter is auto-injected

Usage: atlassian-cli confluence search [OPTIONS] <QUERY>

Arguments:
  <QUERY>


Options:
      --limit <LIMIT>
          Results per page (capped at 50 by the body-expanding search API). With --all, controls first-page batch size

          [default: 50]

      --all
          Fetch all results via cursor pagination

      --stream
          Stream as JSONL (requires --all)

      --expand <EXPAND>
          Expand fields (e.g., body.storage,ancestors)

      --format <FORMAT>
          Format of page bodies in the output

          Possible values:
          - storage:  Storage format, the XHTML Confluence stores
          - markdown: Markdown, converted to and from storage format

          [default: storage]

  -h, --help
          Print help (see a summary with '-h')
```

## `atlassian-cli confluence get`

```text
Fetch a single page by ID

Usage: atlassian-cli confluence get [OPTIONS] <PAGE_ID>

Arguments:
  <PAGE_ID>


Options:
      --format <FORMAT>
          Format of the page body in the output

          Possible values:
          - storage:  Storage format, the XHTML Confluence stores
          - markdown: Markdown, converted to and from storage format

          [default: storage]

  -h, --help
          Print help (see a summary with '-h')
```

## `atlassian-cli confluence create`

```text
Create a page

Usage: atlassian-cli confluence create [OPTIONS] <SPACE> <TITLE> <CONTENT>

Arguments:
  <SPACE>


  <TITLE>


  <CONTENT>
          Page body; `-` reads it from standard input

Options:
      --parent <PARENT>
          Parent page id to nest under (omit to create at the space root)

      --format <FORMAT>
          Format of the page body

          Possible values:
          - storage:  Storage format, the XHTML Confluence stores
          - markdown: Markdown, converted to and from storage format

          [default: storage]

  -h, --help
          Print help (see a summary with '-h')
```

## `atlassian-cli confluence update`

```text
Replace a page's title and body

Usage: atlassian-cli confluence update [OPTIONS] <PAGE_ID> <TITLE> <CONTENT>

Arguments:
  <PAGE_ID>


  <TITLE>


  <CONTENT>
          Page body; `-` reads it from standard input

Options:
      --parent <PARENT>
          Parent page id to re-parent under (omit to keep the current parent)

      --format <FORMAT>
          Format of the page body

          Possible values:
          - storage:  Storage format, the XHTML Confluence stores
          - markdown: Markdown, converted to and from storage format

          [default: storage]

      --base-version <VERSION>
          Version the new body was derived from, as `get` reported it; the update is refused when the page has been saved since

  -h, --help
          Print help (see a summary with '-h')
```

## `atlassian-cli confluence children`

```text
List a page's direct children: pages, folders, whiteboards, databases and embeds

Usage: atlassian-cli confluence children <PAGE_ID>

Arguments:
  <PAGE_ID>


Options:
  -h, --help
          Print help
```

## `atlassian-cli confluence comment`

```text
Read a page's comments and threads, or write footer comments

Usage: atlassian-cli confluence comment <COMMAND>

Commands:
  list     List comments on a page, replies included
  get      Fetch a single comment by id
  replies  List every reply below a comment
  add      Add a footer comment to a page
  update   Update a footer comment's body
  delete   Delete a footer comment by id
  help     Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli confluence comment list`

```text
List comments on a page, replies included

Usage: atlassian-cli confluence comment list [OPTIONS] <PAGE_ID>

Arguments:
  <PAGE_ID>


Options:
      --location <LOCATION>
          Comment family to list (default: both footer and inline)

          [possible values: footer, inline]

      --roots-only
          List only top-level comments, leaving their replies unfetched

      --format <FORMAT>
          Format of comment bodies in the output

          Possible values:
          - storage:  Storage format, the XHTML Confluence stores
          - markdown: Markdown, converted to and from storage format

          [default: storage]

  -h, --help
          Print help (see a summary with '-h')
```

## `atlassian-cli confluence comment get`

```text
Fetch a single comment by id

Usage: atlassian-cli confluence comment get [OPTIONS] <COMMENT_ID>

Arguments:
  <COMMENT_ID>


Options:
      --location <LOCATION>
          Comment family the id belongs to

          [default: footer]
          [possible values: footer, inline]

      --format <FORMAT>
          Format of the comment body in the output

          Possible values:
          - storage:  Storage format, the XHTML Confluence stores
          - markdown: Markdown, converted to and from storage format

          [default: storage]

  -h, --help
          Print help (see a summary with '-h')
```

## `atlassian-cli confluence comment replies`

```text
List every reply below a comment

Usage: atlassian-cli confluence comment replies [OPTIONS] <COMMENT_ID>

Arguments:
  <COMMENT_ID>


Options:
      --location <LOCATION>
          Comment family the id belongs to

          [default: footer]
          [possible values: footer, inline]

      --format <FORMAT>
          Format of reply bodies in the output

          Possible values:
          - storage:  Storage format, the XHTML Confluence stores
          - markdown: Markdown, converted to and from storage format

          [default: storage]

  -h, --help
          Print help (see a summary with '-h')
```

## `atlassian-cli confluence comment add`

```text
Add a footer comment to a page

Usage: atlassian-cli confluence comment add [OPTIONS] <PAGE_ID> <BODY>

Arguments:
  <PAGE_ID>


  <BODY>
          Comment body; `-` reads it from standard input

Options:
      --reply-to <REPLY_TO>
          Reply to an existing comment instead of posting a top-level one

      --format <FORMAT>
          Format of the comment body

          Possible values:
          - storage:  Storage format, the XHTML Confluence stores
          - markdown: Markdown, converted to and from storage format

          [default: storage]

  -h, --help
          Print help (see a summary with '-h')
```

## `atlassian-cli confluence comment update`

```text
Update a footer comment's body

Usage: atlassian-cli confluence comment update [OPTIONS] <COMMENT_ID> <BODY>

Arguments:
  <COMMENT_ID>


  <BODY>
          New comment body; `-` reads it from standard input

Options:
      --format <FORMAT>
          Format of the comment body

          Possible values:
          - storage:  Storage format, the XHTML Confluence stores
          - markdown: Markdown, converted to and from storage format

          [default: storage]

  -h, --help
          Print help (see a summary with '-h')
```

## `atlassian-cli confluence comment delete`

```text
Delete a footer comment by id

Usage: atlassian-cli confluence comment delete <COMMENT_ID>

Arguments:
  <COMMENT_ID>


Options:
  -h, --help
          Print help
```

## `atlassian-cli confluence label`

```text
List, add, or remove labels on a page

Usage: atlassian-cli confluence label <COMMAND>

Commands:
  list    List labels on a page
  add     Add a label to a page
  remove  Remove a label from a page
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli confluence label list`

```text
List labels on a page

Usage: atlassian-cli confluence label list <PAGE_ID>

Arguments:
  <PAGE_ID>


Options:
  -h, --help
          Print help
```

## `atlassian-cli confluence label add`

```text
Add a label to a page

Usage: atlassian-cli confluence label add <PAGE_ID> <LABEL>

Arguments:
  <PAGE_ID>


  <LABEL>


Options:
  -h, --help
          Print help
```

## `atlassian-cli confluence label remove`

```text
Remove a label from a page

Usage: atlassian-cli confluence label remove <PAGE_ID> <LABEL>

Arguments:
  <PAGE_ID>


  <LABEL>


Options:
  -h, --help
          Print help
```

## `atlassian-cli confluence property`

```text
List, set, or delete content properties (structured JSON metadata) on a page

Usage: atlassian-cli confluence property <COMMAND>

Commands:
  list    List content properties on a page
  set     Create or update a content property (value is a JSON literal)
  delete  Delete a content property by key
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli confluence property list`

```text
List content properties on a page

Usage: atlassian-cli confluence property list <PAGE_ID>

Arguments:
  <PAGE_ID>


Options:
  -h, --help
          Print help
```

## `atlassian-cli confluence property set`

```text
Create or update a content property (value is a JSON literal)

Usage: atlassian-cli confluence property set <PAGE_ID> <KEY> <VALUE>

Arguments:
  <PAGE_ID>


  <KEY>


  <VALUE>
          Property value as a JSON literal (e.g. '{"state":"done"}', '42', '"text"'); `-` reads it from standard input

Options:
  -h, --help
          Print help
```

## `atlassian-cli confluence property delete`

```text
Delete a content property by key

Usage: atlassian-cli confluence property delete <PAGE_ID> <KEY>

Arguments:
  <PAGE_ID>


  <KEY>


Options:
  -h, --help
          Print help
```

## `atlassian-cli confluence space`

```text
List spaces, or fetch a single space by key

Usage: atlassian-cli confluence space <COMMAND>

Commands:
  list  List spaces visible to you
  get   Get a single space by key
  help  Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli confluence space list`

```text
List spaces visible to you

Usage: atlassian-cli confluence space list

Options:
  -h, --help
          Print help
```

## `atlassian-cli confluence space get`

```text
Get a single space by key

Usage: atlassian-cli confluence space get <SPACE_KEY>

Arguments:
  <SPACE_KEY>


Options:
  -h, --help
          Print help
```

## `atlassian-cli confluence attachment`

```text
List attachments on a page, or upload a file

Usage: atlassian-cli confluence attachment <COMMAND>

Commands:
  list    List attachments on a page
  upload  Upload a local file as an attachment (creates, or versions by filename)
  help    Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli confluence attachment list`

```text
List attachments on a page

Usage: atlassian-cli confluence attachment list <PAGE_ID>

Arguments:
  <PAGE_ID>


Options:
  -h, --help
          Print help
```

## `atlassian-cli confluence attachment upload`

```text
Upload a local file as an attachment (creates, or versions by filename)

Usage: atlassian-cli confluence attachment upload [OPTIONS] <PAGE_ID> <FILE>

Arguments:
  <PAGE_ID>


  <FILE>
          Path to the local file to upload

Options:
      --comment <COMMENT>
          Optional version comment recorded with the upload

      --minor
          Mark as a minor edit (suppresses watcher notifications on re-upload)

      --content-type <CONTENT_TYPE>
          Override the Content-Type (default: mapped from the file extension)

  -h, --help
          Print help
```

## `atlassian-cli confluence delete`

```text
Move a page to the trash (recoverable — requires --yes)

Usage: atlassian-cli confluence delete [OPTIONS] <PAGE_ID>

Arguments:
  <PAGE_ID>


Options:
      --yes
          Confirm the deletion

  -h, --help
          Print help
```

## `atlassian-cli config`

```text
Create, inspect, or validate the configuration this tool reads

Usage: atlassian-cli config <COMMAND>

Commands:
  init      Create a starter config at --config, the global or project location
  show      Print the resolved config (secrets masked)
  list      List config file paths and environment variable status
  edit      Open the active config file in $EDITOR
  path      Print the path of the active config file
  validate  Validate configured credentials against Atlassian auth/API endpoints
  help      Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli config init`

```text
Create a starter config at --config, the global or project location

Usage: atlassian-cli config init [OPTIONS]

Options:
      --global
          Write the global config instead of --config or ./.atlassian.toml

  -h, --help
          Print help
```

## `atlassian-cli config show`

```text
Print the resolved config (secrets masked)

Usage: atlassian-cli config show

Options:
  -h, --help
          Print help
```

## `atlassian-cli config list`

```text
List config file paths and environment variable status

Usage: atlassian-cli config list

Options:
  -h, --help
          Print help
```

## `atlassian-cli config edit`

```text
Open the active config file in $EDITOR

Usage: atlassian-cli config edit [OPTIONS]

Options:
      --global
          Edit the global config instead of --config or the project config

  -h, --help
          Print help
```

## `atlassian-cli config path`

```text
Print the path of the active config file

Usage: atlassian-cli config path [OPTIONS]

Options:
      --global
          Print the global config path instead of --config or the project config

  -h, --help
          Print help
```

## `atlassian-cli config validate`

```text
Validate configured credentials against Atlassian auth/API endpoints

Usage: atlassian-cli config validate

Options:
  -h, --help
          Print help
```

## `atlassian-cli auth`

```text
Sign in, sign out, and report or refresh the stored session

Usage: atlassian-cli auth <COMMAND>

Commands:
  login    Start the OAuth 3LO flow and persist tokens for the active profile
  logout   Discard stored OAuth tokens for the active profile
  status   Show the active profile's stored token status (identity, expiry, scopes)
  refresh  Force-refresh the access_token using the stored refresh_token
  help     Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli auth login`

```text
Start the OAuth 3LO flow and persist tokens for the active profile

Usage: atlassian-cli auth login [OPTIONS]

Options:
      --no-browser
          Print the authorize URL instead of opening a browser

  -h, --help
          Print help
```

## `atlassian-cli auth logout`

```text
Discard stored OAuth tokens for the active profile

Usage: atlassian-cli auth logout

Options:
  -h, --help
          Print help
```

## `atlassian-cli auth status`

```text
Show the active profile's stored token status (identity, expiry, scopes)

Usage: atlassian-cli auth status

Options:
  -h, --help
          Print help
```

## `atlassian-cli auth refresh`

```text
Force-refresh the access_token using the stored refresh_token

Usage: atlassian-cli auth refresh

Options:
  -h, --help
          Print help
```

## `atlassian-cli self`

```text
Inspect, update, or remove this installation

Usage: atlassian-cli self <COMMAND>

Commands:
  status     Report what this installation is and where its files are
  update     Replace this binary with a published release
  skill      Install or remove the Claude Code skill this binary carries
  uninstall  Remove this binary, the skill it deployed, and the tokens it stored
  help       Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli self status`

```text
Report what this installation is and where its files are

Usage: atlassian-cli self status

Options:
  -h, --help
          Print help
```

## `atlassian-cli self update`

```text
Replace this binary with a published release

Usage: atlassian-cli self update [OPTIONS]

Options:
      --version <VER>
          Install this version instead of the latest. Named explicitly, so it may go back

      --force
          Replace the binary even when it already reports the target version

      --verify-attestations
          Additionally hold the archive to the build provenance the release attested

  -h, --help
          Print help
```

## `atlassian-cli self skill`

```text
Install or remove the Claude Code skill this binary carries

Usage: atlassian-cli self skill <COMMAND>

Commands:
  install  Write the skill this binary carries into ~/.claude/skills
  remove   Remove the deployed skill
  help     Print this message or the help of the given subcommand(s)

Options:
  -h, --help
          Print help
```

## `atlassian-cli self skill install`

```text
Write the skill this binary carries into ~/.claude/skills

Usage: atlassian-cli self skill install

Options:
  -h, --help
          Print help
```

## `atlassian-cli self skill remove`

```text
Remove the deployed skill

Usage: atlassian-cli self skill remove [OPTIONS]

Options:
      --yes
          Confirm the removal

  -h, --help
          Print help
```

## `atlassian-cli self uninstall`

```text
Remove this binary, the skill it deployed, and the tokens it stored

Usage: atlassian-cli self uninstall [OPTIONS]

Options:
      --yes
          Confirm the removal

      --keep-skill
          Leave the deployed skill in place

      --keep-credentials
          Leave stored OAuth tokens in the keychain and the credentials file

      --purge-config
          Also remove the global configuration directory

  -h, --help
          Print help
```

## `atlassian-cli completions`

```text
Generate a shell completion script on stdout

Usage: atlassian-cli completions <SHELL>

Arguments:
  <SHELL>
          Target shell

          [possible values: bash, elvish, fish, powershell, zsh]

Options:
  -h, --help
          Print help
```
