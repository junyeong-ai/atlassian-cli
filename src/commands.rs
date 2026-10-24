pub(crate) mod confluence;
pub(crate) mod jira;

use anyhow::{Context, Result};

/// A body argument, with `-` standing for standard input.
///
/// Empty input is refused: `-` with nothing piped in is a forgotten redirect,
/// and taking it as the body would blank a page or post an empty comment.
pub(crate) async fn read_body(argument: String) -> Result<String> {
    if argument != "-" {
        return Ok(argument);
    }
    let body = tokio::task::spawn_blocking(|| std::io::read_to_string(std::io::stdin()))
        .await?
        .context("Failed to read the body from standard input")?;
    if body.is_empty() {
        anyhow::bail!(
            "`-` reads the body from standard input, but nothing was piped or redirected into it"
        );
    }
    Ok(body)
}
