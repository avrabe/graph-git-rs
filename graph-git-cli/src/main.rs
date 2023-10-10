use clap::{crate_version, Parser};
use convenient_bitbake::Bitbake;
use convenient_git::GitRepository;
use convenient_kas::KasManifest;
use convenient_repo::find_repo_manifest;
use graph_git::{
    merge_link, merge_node, node_commit, node_kas_manifest, node_message, node_person,
    node_reference, node_repository, GraphDatabase, delete_reference,
};
use neo4rs::Query;
use std::sync::Arc;
use tempfile::tempdir;
use tracing::{error, info, span, warn, Level};
use tracing_subscriber::FmtSubscriber;

use std::collections::{HashMap, VecDeque, HashSet};
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
async fn iterate_through_branches(git_repository: &GitRepository, queue: &Queue, graph: &GraphDatabase) -> Vec<Query> {
    let span = span!(Level::INFO, "iterate", value = git_repository.git_url);
    let _enter = span.enter();
    let mut collector = Vec::<Query>::new();
    // Get the branches that are already in the database
    let git_branches_in_db_initial =  graph.query_branches_for_repository(&git_repository.git_url).await;
    let mut git_branches_in_db =  git_branches_in_db_initial.clone();

    add_git_repository(&mut collector, git_repository);

    for branch in git_repository.get_remote_heads().unwrap().iter() {
        let branch_name = branch.name.as_str();
        // Remove the branch from the list of branches in the database still to process
        git_branches_in_db.remove(branch_name);
        add_branches_to_query_on_branch(&mut collector, branch, git_repository);
        add_head_commit_to_query_on_branch(git_repository, branch, &mut collector, &span);
        match git_repository.checkout(branch_name) {
            Ok(_) => {
                find_kas_manifests_in_directory(
                    git_repository,
                    &span,
                    &mut collector,
                    branch,
                    queue,
                );
                find_bitbake_manifests_on_branch(git_repository);
                find_repo_manifest_on_branch(git_repository);
            }
            Err(e) => {
                error!(parent: &span, "Error: {}", e);
            }
        }
    }
    remove_branches_from_database(&mut collector, git_branches_in_db, git_branches_in_db_initial, &git_repository.git_url);
    collector
}


fn remove_branches_from_database(collector: &mut Vec<Query>, branches_to_remove: HashSet<String>,branches_to_remove_initial: HashSet<String>, repository_url: &String) {
    let span = span!(Level::INFO, "remove_branches_from_database", value = repository_url);
    let _enter = span.enter();
    info!(parent: &span, "Removing branches from database: {}/{} items to remove", branches_to_remove.len(), branches_to_remove_initial.len());
    info!(parent: &span, "Initial branches are: {:?}", branches_to_remove_initial);
    
    for branch in branches_to_remove.iter() {
        let branch_name = branch.clone();
        collector.push(delete_reference(node_reference(&branch_name, repository_url)));
        info!(parent: &span, "Removed branch {} from database", branch_name);
    }
}

fn add_git_repository(collector: &mut Vec<Query>, git_repository: &GitRepository) {
    let git_url: &String = &git_repository.git_url;

    collector.push(merge_node(node_repository(git_url)));
}

fn add_branches_to_query_on_branch(
    collector: &mut Vec<Query>,
    branch: &convenient_git::GitRemoteHead,
    git_repository: &GitRepository,
) {
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

fn add_head_commit_to_query_on_branch(
    git_repository: &GitRepository,
    branch: &convenient_git::GitRemoteHead,
    collector: &mut Vec<Query>,
    span: &tracing::Span,
) {
    let branch_name = branch.name.as_str();
    let span = span!(parent: span, Level::INFO, "head_commit", value=branch_name);

    let commit = git_repository.find_reference(branch_name);
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
            error!(parent: span, "Error: {}", branch_name);
        }
    }
}

fn find_kas_manifests_in_directory(
    git_repository: &GitRepository,
    parent_span: &tracing::Span,
    collector: &mut Vec<Query>,
    branch: &convenient_git::GitRemoteHead,
    queue: &Queue,
) {
    let branch_name = branch.name.as_str();
    span!(parent: parent_span, Level::INFO, "kas", value=branch_name).in_scope(|| {
        // perform some work in the context of `my_span`...

        let kas_manifests = KasManifest::find_kas_manifest(
            git_repository.repo.as_ref().unwrap().workdir().unwrap(),
        );
        info!("Found {} kas manifest(s)", kas_manifests.len());
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
                                info!(
                                    "Found kas {} repository {}",
                                    kas_repository_name,
                                    url.as_str()
                                );
                            }
                            None => {
                                error!("Error: {}", kas_repository_name);
                            }
                        };
                        match repository.refspec {
                            Some(refspec) => {
                                collector
                                    .push(merge_node(node_reference(refspec.as_str(), &git_repo)));
                                collector.push(merge_link(
                                    node_repository(&git_repo),
                                    node_reference(refspec.as_str(), &git_repo),
                                    "has".to_string(),
                                ));
                                collector.push(merge_link(
                                    node_kas_manifest(kas.path.as_str(), branch.oid.as_str()),
                                    node_reference(refspec.as_str(), &git_repo),
                                    "refers".to_string(),
                                ));
                                info!(
                                    "Found kas {} refspec {}",
                                    kas_repository_name,
                                    refspec.as_str()
                                );
                            }
                            None => {
                                error!(
                                    "Error: {}. Need to find a way for default refspec.",
                                    kas_repository_name
                                );
                            }
                        };
                    }

                    None => {
                        error!("Error: {}", kas_repository_name);
                    }
                }
            }
        }
    }); // --> Subscriber::exit(my_span)
}

fn find_bitbake_manifests_on_branch(git_repository: &GitRepository) {
    let _bitbake_manifests =
        Bitbake::find_bitbake_manifest(git_repository.repo.as_ref().unwrap().workdir().unwrap());
}

fn find_repo_manifest_on_branch(git_repository: &GitRepository) {
    let _bitbake_manifests =
        find_repo_manifest(git_repository.repo.as_ref().unwrap().workdir().unwrap());
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

    queue.add(opts.git_url.clone());
    let graph: GraphDatabase = GraphDatabase::new(opts.uri, opts.user, opts.password, opts.db).await;

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

        collector.append(&mut iterate_through_branches(&git_repository, &queue, &graph).await);

        tmp_dir.close().unwrap();
    }

    graph.txn_run_queries(collector).await.unwrap();


}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use convenient_git::GitRemoteHead;
    use tracing_test::traced_test;

    use super::*;

    #[test]

    fn test_add_and_take() {
        let obj = Queue::new();

        // should return item.
        obj.add("item".to_string());
        assert_eq!(obj.take(), Some("item".to_string()));

        // Should only return item2 as item was already processed.
        obj.add("item2".to_string());
        obj.add("item".to_string());
        assert_eq!(obj.take(), Some("item2".to_string()));
    }

    #[test]
    fn add_git_repository_test() {
        let mut queries = Vec::new();
        let test_repo = GitRepository {
            repo: None,
            git_url: "foo:bar:foo".to_string(),
        };
        add_git_repository(&mut queries, &test_repo);
        assert_eq!(queries.len(), 1);
    }

    #[traced_test]
    #[test]
    fn add_head_commit_to_query_on_branch_test() {
        let binding = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let d = binding.parent().unwrap();

        let mut collector = Vec::<Query>::new();

        let span = tracing::info_span!("add_head_commit_to_query_on_branch");
        let test_repo = GitRepository {
            repo: Some(git2::Repository::open(d).unwrap()),
            git_url: "foo:bar:foo".to_string(),
        };
        let branch = GitRemoteHead {
            name: "main".to_string(),
            oid: "foo:bar:foo".to_string(),
        };
        add_head_commit_to_query_on_branch(&test_repo, &branch, &mut collector, &span);
        assert_eq!(collector.len(), 4);
    }

    #[traced_test]
    #[test]
    fn find_kas_manifests_in_directory_test() {
        let binding = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let d = binding.parent().unwrap();
        let queue = Arc::new(Queue::new());

        let mut collector = Vec::<Query>::new();

        let span = tracing::info_span!("find_kas_manifests_in_directory");
        let test_repo = GitRepository {
            repo: Some(git2::Repository::open(d).unwrap()),
            git_url: "foo:bar:foo".to_string(),
        };
        let branch = GitRemoteHead {
            name: "main".to_string(),
            oid: "foo:bar:foo".to_string(),
        };
        find_kas_manifests_in_directory(&test_repo, &span, &mut collector, &branch, &queue);
        assert_eq!(collector.len(), 0);
    }
}
