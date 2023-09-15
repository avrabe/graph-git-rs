use git2::{FetchOptions, Repository, BranchType};
use neo4rs::{query, Graph, Node, Query};
use std::{path::Path, sync::Arc};
use tempfile::tempdir;

#[tokio::main]
async fn main() {
    let mut collector = Vec::<Query>::new();
    let mut builder = git2::build::RepoBuilder::new();
    builder.bare(true);

    let tmp_dir = tempdir().unwrap();
    let file_path = tmp_dir.path().join("repo");

    let repo_path = file_path.as_path();
    println!("Repo path: {}", repo_path.display());
    let url = "https://github.com/avrabe/meta-fmu.git";

    // Try opening the repository
    let repo = match Repository::open(repo_path) {
        Ok(repo) => repo,
        Err(e) if e.code() == git2::ErrorCode::NotFound => {
            // Repository not found, clone it
            builder.clone(url, Path::new(repo_path)).unwrap()
        }
        Err(e) => {
            // Some other error, panic
            panic!("failed to open: {}", e);
        }
    };

    collector.push(query(
        format!("MERGE (p:Repository {{uri: \'{}\'}})", url).as_str(),
    ));

    let mut fo = FetchOptions::new();
    //let fo = FetchOptions::default().download_tags(git2::AutotagOption::All);

    // Repository exists, try to update it
    match repo.find_remote("origin") {
        Ok(mut remote) => {
            remote.connect(git2::Direction::Fetch).unwrap();
            remote.download(&[] as &[&str], Some(&mut fo)).unwrap();
            let _ = remote.disconnect();
            //repo.set_head("origin/master").unwrap();
            //repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))
            //    .unwrap();
        }
        Err(e) if e.code() == git2::ErrorCode::NotFound => {
            // No origin remote found
            println!("No remote found");
        }
        Err(e) => {
            // Some other error
            println!("Error: {}", e);
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

            let name = branch.name().split('/').last().unwrap();

            
            collector.push(query(
                format!("MERGE (p:Commit {{oid: \'{}\'}})", branch.oid()).as_str(),
            ));
            collector.push(query(
                format!("MERGE (p:Reference {{name: \'{}\'}})", name).as_str(),
            ));
            collector.push(query(
                format!(
                    "
                MATCH (c:Repository {{uri: \'{}\'}})
                MATCH (r:Reference {{name: \'{}\'}})
                MERGE (c)-[:has ]->(r)",
                    url,
                    name
                )
                .as_str(),
            ));
            collector.push(query(
                format!(
                    "
                MATCH (c:Commit {{oid: \'{}\'}})
                MATCH (r:Reference {{name: \'{}\'}})
                MERGE (c)-[:links_to ]->(r)",
                    branch.oid(),
                    name
                )
                .as_str(),
            ));
            let remote_name = format!("refs/remotes/origin/{}",name);
            let reference = repo.find_reference(remote_name.as_str());
            match reference {
                Ok(reference) => {
                    let commit = reference.peel_to_commit().unwrap();
                    let author = commit.author();
                    let name = author.name().unwrap();
                    let email = author.email().unwrap();
                    let message = commit.message().unwrap();
                    collector.push(query(
                        format!(
                            "MERGE (p:Person {{email: \'{}\', name: \'{}\'}})",
                            email, name
                        )
                        .as_str(),
                    ));
                    collector.push(query(
                        format!("MERGE (p:Message {{message: \'{}\'}})", message).as_str(),
                    ));
                    collector.push(query(
                        format!(
                            "
                        MATCH (c:Commit {{oid: \'{}\'}})
                        MATCH (p:Person {{email: \'{}\', name: \'{}\'}})
                        MERGE (c)-[:authored_by ]->(p)",
                            branch.oid(),
                            email,
                            name
                        )
                        .as_str(),
                    ));
                    collector.push(query(
                        format!(
                            "
                        MATCH (c:Commit {{oid: \'{}\'}})
                        MATCH (p:Message {{message: \'{}\'}})
                        MERGE (c)-[:has_message ]->(p)",
                            branch.oid(),
                            message,
                        )
                        .as_str(),
                    ));
                }
                Err(e) => {
                    println!("Error: {}", e);
                }
            }
        }

        println!("{}\t{}", branch.oid(), branch.name());
    }

    // concurrent queries
    let uri = "neo4j://127.0.0.1:7687";
    let user = "neo4j";
    let pass = "12345678";
    let graph = Arc::new(Graph::new(uri, user, pass).await.unwrap());
    for _ in 1..=2 {
        let graph = graph.clone();
        tokio::spawn(async move {
            let mut result = graph
                .execute(query("MATCH (p:Person {name: $name}) RETURN p").param("name", "mark"))
                .await
                .unwrap();
            while let Ok(Some(row)) = result.next().await {
                let node: Node = row.get("p").unwrap();
                let name: String = node.get("name").unwrap();
                println!("{}", name);
            }
        });
    }

    //Transactions
    let txn = graph.start_txn().await.unwrap();
    txn.run_queries(
        collector, //        vec![
                  //        query("CREATE (p:Person {name: 'mark'})"),
                  //        query("CREATE (p:Person {name: 'jake'})"),
                  //        query("CREATE (p:Person {name: 'luke'})"),
                  //    ]
    )
    .await
    .unwrap();
    txn.commit().await.unwrap(); //or txn.rollback().await.unwrap();

    // `tmp_dir` goes out of scope, the directory as well as
    // `tmp_file` will be deleted here.
    tmp_dir.close().unwrap();
}
