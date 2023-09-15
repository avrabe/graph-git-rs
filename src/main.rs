use clap::Parser;
use git2::{BranchType, FetchOptions, Oid, Repository};
use neo4rs::{query, ConfigBuilder, Graph, Query};
use std::{path::Path, sync::Arc};
use tempfile::tempdir;
use tracing::{debug, error, info, Level, warn};
use tracing_subscriber::FmtSubscriber;

/// This doc string acts as a help message when the user runs '--help'
/// as do all doc strings on fields
#[derive(Parser)]
#[clap(version = "1.0", author = "Ralf Anton Beier")]
struct Opts {
    /// The path to the neo4j repository.
    #[clap(
        short,
        long,
        default_value = "neo4j://127.0.0.1:7687",
        env = "NEO4J_URI"
    )]
    uri: String,

    /// The username for authenticating with the Neo4j server.
    #[clap(short = 'r', long, default_value = "neo4j", env = "NEO4J_USER")]
    user: String,

    /// The password for authenticating with the Neo4j server.
    #[clap(
        short,
        long,
        default_value = "12345678",
        env = "NEO4J_PASSWORD",
        hide_env_values = true
    )]
    password: String,

    /// The name of the database to connect to.
    /// Defaults to "graph" if not set.
    #[clap(short = 'b', long, default_value = "graph", env = "NEO4J_DB")]
    db: String,

    #[clap(
        short = 'g',
        long,
        default_value = "https://github.com/avrabe/meta-fmu.git"
    )]
    git_url: String,

    /// Print debug information
    #[clap(short)]
    debug: bool,
}

/// Gets the log level enum variant from a level string.
///
/// # Arguments
///
/// * `level` - The log level string, e.g. "DEBUG", "INFO".
///
/// # Returns
///
/// Returns the corresponding `Level` enum variant for the level string.
///
/// # Examples
///
/// ```
/// let level = get_log_level("INFO");
/// assert_eq!(level, Level::INFO);
/// ```
pub fn get_log_level(level: &str) -> Level {
    match level.to_uppercase().as_ref() {
        "DEBUG" => Level::DEBUG,
        "INFO" => Level::INFO,
        "WARN" => Level::WARN,
        "ERROR" => Level::ERROR,
        "FATAL" => Level::ERROR,
        _ => Level::INFO,
    }
}

/// Creates a Cypher node representation for a Git reference.
///
/// # Arguments
///
/// * `name` - The name of the reference
///
fn node_reference(name: &str) -> String {
    format!("(reference:Reference {{name: \'{}\'}})", name)
}

/// Creates a Cypher node representation for a Git commit.
///
/// # Arguments
///
/// * `oid` - The object ID of the commit
fn node_commit(oid: Oid) -> String {
    format!("(commit:Commit {{oid: \'{}\'}})", oid)
}

/// Creates a Cypher node representation for a Git repository.
///
/// # Arguments
///
/// * `uri` - The URI of the repository
fn node_repository(uri: &str) -> String {
    format!("(repository:Repository {{uri: '{}'}})", uri)
}

/// Creates a Cypher node representation for a Git person.
///
/// # Arguments
///
/// * `name` - The name of the person
fn node_person(name: &str, email: &str) -> String {
    format!(
        "(person:Person {{name: \'{}\', email: \'{}\'}})",
        name, email
    )
}

/// Creates a Cypher node representation for a Git message.
///
/// # Arguments
///
/// * `message` - The message of the commit
fn node_message(message: &str) -> String {
    format!("(message:Message {{message: \'{}\'}})", message)
}

/// Creates a Cypher query to merge a node.
///
/// # Arguments
///
/// * `node` - The node to merge
fn merge_node(node: String) -> Query {
    let q = format!("MERGE {}", node);
    debug!("{}", q);
    query(q.as_str())
}

/// Creates a Cypher query to merge a link.
///
/// # Arguments
///
/// * `from` - The source node
/// * `to` - The target node
/// * `link` - The link between the nodes
fn merge_link(from: String, to: String, link: String) -> Query {
    let q = format!("MERGE {}-[:{}]->{}", from, link, to);
    debug!("{}", q);
    query(q.as_str())
}

#[tokio::main]
async fn main() {
    // Get the command line arguments
    let opts: Opts = Opts::parse();

    let log_level = if opts.debug {
        "debug".to_string()
    } else {
        "warn".to_string()
    };

    // a builder for `FmtSubscriber`.
    let subscriber = FmtSubscriber::builder()
        // all spans/events with a level higher than TRACE (e.g, debug, info, warn, etc.)
        // will be written to stdout.
        .with_max_level(get_log_level(&log_level))
        // completes the builder.
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    let mut collector = Vec::<Query>::new();
    let mut builder = git2::build::RepoBuilder::new();
    builder.bare(true);

    let tmp_dir = tempdir().unwrap();
    let file_path = tmp_dir.path().join("repo");

    let repo_path = file_path.as_path();
    warn!("Repo path: {}", repo_path.display());
    //let opts = "https://github.com/avrabe/meta-fmu.git";

    // Try opening the repository
    let repo = match Repository::open(repo_path) {
        Ok(repo) => repo,
        Err(e) if e.code() == git2::ErrorCode::NotFound => {
            // Repository not found, clone it
            builder.clone(&opts.git_url, Path::new(repo_path)).unwrap()
        }
        Err(e) => {
            // Some other error, panic
            panic!("failed to open: {}", e);
        }
    };

    collector.push(merge_node(node_repository(&opts.git_url)));

    let mut fo = FetchOptions::new();
    //let fo = FetchOptions::default().download_tags(git2::AutotagOption::All);

    // Repository exists, try to update it
    match repo.find_remote("origin") {
        Ok(mut remote) => {
            remote.connect(git2::Direction::Fetch).unwrap();
            remote.download(&[] as &[&str], Some(&mut fo)).unwrap();
            let _ = remote.disconnect();
        }
        Err(e) if e.code() == git2::ErrorCode::NotFound => {
            // No origin remote found
            error!("No remote found");
        }
        Err(e) => {
            // Some other error
            error!("Error: {}", e);
        }
    };

    let mut remote = repo.find_remote("origin").unwrap();
    remote.connect(git2::Direction::Fetch).unwrap();

    for branch in remote.list().unwrap() {
        if branch.name().starts_with("refs/heads") {
            let branch_name = branch.name().to_string();
            let branch_ref = branch.oid();
            let branch_commit = repo.find_commit(branch_ref).unwrap();

            if let Ok(local_branch) = repo.find_branch(&branch_name, BranchType::Local) {
                if !local_branch.is_head() {
                    repo.branch(&branch_name, &branch_commit, true).unwrap();
                }
            } else {
                repo.branch(&branch_name, &branch_commit, true).unwrap();
            }
        }
    }
    for branch in remote.list().unwrap().iter() {
        if branch.name().starts_with("refs/heads") {
            let name = branch.name().split("refs/heads/").last().unwrap();

            collector.push(merge_node(node_commit(branch.oid())));
            collector.push(merge_node(node_reference(name)));
            collector.push(merge_link(
                node_repository(&opts.git_url),
                node_reference(name),
                "has".to_string(),
            ));
            collector.push(merge_link(
                node_commit(branch.oid()),
                node_reference(name),
                "links_to".to_string(),
            ));

            let remote_name = format!("refs/remotes/origin/{}", name);
            let reference = repo.find_reference(remote_name.as_str());
            match reference {
                Ok(reference) => {
                    let commit = reference.peel_to_commit().unwrap();
                    let author = commit.author();
                    let name = author.name().unwrap();
                    let email = author.email().unwrap();
                    let message = commit.message().unwrap();

                    collector.push(merge_node(node_person(name, email)));
                    collector.push(merge_node(node_message(message)));
                    collector.push(merge_link(
                        node_commit(branch.oid()),
                        node_person(name, email),
                        "authored_by".to_string(),
                    ));

                    collector.push(merge_link(
                        node_commit(branch.oid()),
                        node_message(message),
                        "has_message".to_string(),
                    ));
                }
                Err(e) => {
                    error!("Error: {}", e);
                }
            }
            info!("process {}\t{} as {}", branch.oid(), branch.name(), name);
        } else {
            info!("skip {}\t{}", branch.oid(), branch.name());
        }
    }

    // concurrent queries
    let config = ConfigBuilder::new()
        .uri(opts.uri)
        .user(opts.user)
        .password(opts.password)
        .db(opts.db)
        .build()
        .unwrap();
    let graph = Arc::new(Graph::connect(config).await.unwrap());

    //for _ in 1..=2 {
    //    let graph = graph.clone();
    //    tokio::spawn(async move {
    //        let mut result = graph
    //            .execute(query("MATCH (p:Person {name: $name}) RETURN p").param("name", "mark"))
    //            .await
    //            .unwrap();
    //        while let Ok(Some(row)) = result.next().await {
    //            let node: Node = row.get("p").unwrap();
    //            let name: String = node.get("name").unwrap();
    //            println!("{}", name);
    //        }
    //    });
    //}

    //Transactions
    let txn = graph.start_txn().await.unwrap();
    txn.run_queries(collector).await.unwrap();
    txn.commit().await.unwrap(); //or txn.rollback().await.unwrap();

    // `tmp_dir` goes out of scope, the directory as well as
    // `tmp_file` will be deleted here.
    tmp_dir.close().unwrap();
}
