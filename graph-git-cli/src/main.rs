use clap::{crate_version, Parser};
use convenient_git::GitRepository;
use convenient_kas::KasManifest;
use graph_git::{
    merge_link, merge_node, node_commit, node_kas_manifest, node_message, node_person,
    node_reference, node_repository,
};
use neo4rs::{ConfigBuilder, Graph, Query};
use std::sync::Arc;
use tempfile::tempdir;
use tracing::{error, info, span, warn, Level};
use tracing_subscriber::FmtSubscriber;

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

struct Queue {
    queue: Mutex<VecDeque<String>>,
    dict: Mutex<HashMap<String, String>>,
}

impl Queue {
    fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            dict: Mutex::new(HashMap::new()),
        }
    }

    fn add(&self, item: String) {
        let mut queue = self.queue.lock().unwrap();
        let mut dict = self.dict.lock().unwrap();
        if dict.contains_key(&item) {
            info!("Already in queue: {}", item);
        } else {
            dict.insert(item.clone(), item.clone());
            queue.push_back(item);
        }
    }

    fn take(&self) -> Option<String> {
        let mut queue = self.queue.lock().unwrap();
        queue.pop_front()
    }
}

/// Options for the application.
#[derive(Parser)]
#[clap(version = crate_version!(), author = "Ralf Anton Beier")]
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

/// Adds queries to find all branches in the Git repository.
///
/// # Arguments
///
/// * `git_repository` - The GitRepository struct containing the repository to query.
///
/// # Returns
///
/// A Vec&lt;Query&gt; containing one query per branch to find all branches in the repository.
fn add_branches_to_query(git_repository: &GitRepository) -> Vec<Query> {
    let span = span!(Level::INFO, "add_branches_to_query");
    let _enter = span.enter();
    let mut collector = Vec::<Query>::new();
    for branch in git_repository.get_remote_heads().unwrap().iter() {
        collector.push(merge_node(node_commit(branch.oid.as_str())));
        collector.push(merge_node(node_reference(
            branch.name.as_str(),
            &git_repository.git_url,
        )));
        collector.push(merge_link(
            node_repository(&git_repository.git_url),
            node_reference(branch.name.as_str(), &git_repository.git_url),
            "has".to_string(),
        ));
        collector.push(merge_link(
            node_commit(branch.oid.as_str()),
            node_reference(branch.name.as_str(), &git_repository.git_url),
            "links_to".to_string(),
        ));
    }
    collector
}

fn add_head_commit_to_query(git_repository: &GitRepository) -> Vec<Query> {
    let span = span!(Level::INFO, "add_head_commit_to_query");
    let _enter = span.enter();
    let mut collector = Vec::<Query>::new();
    for branch in git_repository.get_remote_heads().unwrap().iter() {
        let commit = git_repository.find_reference(branch.name.as_str());
        match commit {
            Some(commit) => {
                collector.push(merge_node(node_person(
                    commit.name.as_str(),
                    commit.email.as_str(),
                )));
                collector.push(merge_node(node_message(commit.message.as_str())));
                collector.push(merge_link(
                    node_commit(branch.oid.as_str()),
                    node_person(commit.name.as_str(), commit.email.as_str()),
                    "authored_by".to_string(),
                ));

                collector.push(merge_link(
                    node_commit(branch.oid.as_str()),
                    node_message(commit.message.as_str()),
                    "has_message".to_string(),
                ));
            }
            None => {
                error!(parent: &span, "Error: {}", branch.name.as_str());
            }
        }
    }
    collector
}

fn find_kas_manifests_in_branches(git_repository: &GitRepository, queue: &Queue) -> Vec<Query> {
    let span = span!(Level::INFO, "add_head_commit_to_query");
    let _enter = span.enter();
    let mut collector = Vec::<Query>::new();
    for branch in git_repository.get_remote_heads().unwrap().iter() {
        match git_repository.checkout(branch.name.as_str()) {
            Ok(_) => {
                let kas_manifests = KasManifest::find_kas_manifest(
                    git_repository.repo.as_ref().unwrap().workdir().unwrap(),
                );
                info!(parent: &span, "Found {} kas manifest(s)", kas_manifests.len());
                for kas in kas_manifests {
                    collector.push(merge_node(node_kas_manifest(
                        kas.path.as_str(),
                        branch.oid.as_str(),
                    )));
                    collector.push(merge_link(
                        node_commit(branch.oid.as_str()),
                        node_kas_manifest(kas.path.as_str(), branch.oid.as_str()),
                        "contains".to_string(),
                    ));
                    for (kas_repository_name, kas_repository) in kas.manifest.repos {
                        let mut git_repo: String = String::new();
                        // if no repo was given. Assume the current repo
                        git_repo.push_str(&git_repository.git_url);
                        match kas_repository {
                            Some(repository) => {
                                match repository.url {
                                    Some(url) => {
                                        queue.add(url.clone());
                                        collector.push(merge_node(node_repository(url.as_str())));
                                        git_repo.replace_range(.., url.as_str());
                                        info!(parent: &span, "Found kas {} repository {}", kas_repository_name, url.as_str());
                                    }
                                    None => {
                                        error!(parent: &span, "Error: {}", kas_repository_name);
                                    }
                                };
                                match repository.refspec {
                                    Some(refspec) => {
                                        collector.push(merge_node(node_reference(
                                            refspec.as_str(),
                                            &git_repo,
                                        )));
                                        collector.push(merge_link(
                                            node_repository(&git_repo),
                                            node_reference(refspec.as_str(), &git_repo),
                                            "has".to_string(),
                                        ));
                                        collector.push(merge_link(
                                            node_kas_manifest(
                                                kas.path.as_str(),
                                                branch.oid.as_str(),
                                            ),
                                            node_reference(refspec.as_str(), &git_repo),
                                            "refers".to_string(),
                                        ));
                                        info!(parent: &span, "Found kas {} refspec {}", kas_repository_name, refspec.as_str());
                                    }
                                    None => {
                                        error!(parent: &span, "Error: {}. Need to find a way for default refspec.", kas_repository_name);
                                    }
                                };
                            }

                            None => {
                                error!(parent: &span, "Error: {}", kas_repository_name);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                error!(parent: &span, "Error: {}", e);
            }
        }
    }
    collector
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
        .with_file(true)
        // completes the builder.
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    let mut collector = Vec::<Query>::new();
    let queue = Arc::new(Queue::new());

    queue.add(opts.git_url);

    while let Some(git_url) = queue.take() {
        info!("Preparing: {}", git_url);
        let tmp_dir = tempdir().unwrap();
        let file_path = tmp_dir.path().join("repo");

        let repo_path = file_path.as_path();
        warn!("Repo path: {}", repo_path.display());

        // Try opening the repository
        let git_repository = GitRepository::new(repo_path, &git_url);
        //let repo = gitRepository.new_repository(repo_path, &opts.git_url);
        git_repository.update_from_remote();
        git_repository.map_remote_branches_local();

        collector.push(merge_node(node_repository(&git_url)));
        collector.append(&mut add_branches_to_query(&git_repository));
        collector.append(&mut add_head_commit_to_query(&git_repository));
        collector.append(&mut find_kas_manifests_in_branches(&git_repository, &queue));
        tmp_dir.close().unwrap();
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
    //Transactions
    let txn = graph.start_txn().await.unwrap();
    txn.run_queries(collector).await.unwrap();
    txn.commit().await.unwrap();

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

    // `tmp_dir` goes out of scope, the directory as well as
    // `tmp_file` will be deleted here.
}
