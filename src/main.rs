use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    github_local_indexer::run().await
}
