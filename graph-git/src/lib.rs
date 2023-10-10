use std::{collections::HashSet, error::Error, sync::Arc};

use neo4rs::{query, ConfigBuilder, Graph, Node, Query};
use tracing::{debug, error};

/// Creates a Cypher node representation for a Git reference.
///
/// # Arguments
///
/// * `name` - The name of the reference
///
pub fn node_reference(name: &str, uri: &str) -> GitCypher {
    GitCypher {
        var: "reference".to_owned(),
        cypher: format!(
            "(reference:Reference {{name: \'{}\', uri: '{}'}})",
            name, uri
        ),
    }
}

/// Creates a Cypher node representation for a Git commit.
///
/// # Arguments
///
/// * `oid` - The object ID of the commit
pub fn node_commit(oid: &str) -> GitCypher {
    GitCypher {
        var: "commit".to_owned(),
        cypher: format!("(commit:Commit {{oid: \'{}\'}})", oid),
    }
}

/// Creates a Cypher node representation for a Git repository.
///
/// # Arguments
///
/// * `uri` - The URI of the repository
pub fn node_repository(uri: &str) -> GitCypher {
    GitCypher {
        var: "repository".to_owned(),
        cypher: format!("(repository:Repository {{uri: '{}'}})", uri),
    }
}

pub fn node_kas_manifest(path: &str, oid: &str) -> GitCypher {
    GitCypher {
        var: "repository".to_owned(),
        cypher: format!(
            "(repository:Manifest {{path: '{}', oid: \'{}\', type: 'kas'}})",
            path, oid
        ),
    }
}

/// Creates a Cypher node representation for a Git person.
///
/// # Arguments
///
/// * `name` - The name of the person
pub fn node_person(name: &str, email: &str) -> GitCypher {
    let name = name.replace('\'', "\\\'");
    let email = email.replace('\'', "\\\'");
    GitCypher {
        var: "person".to_owned(),
        cypher: format!(
            "(person:Person {{name: \'{}\', email: \'{}\'}})",
            name, email
        ),
    }
}

pub struct GitCypher {
    pub var: String,
    pub cypher: String,
}

/// Creates a Cypher node representation for a Git message.
///
/// # Arguments
///
/// * `message` - The message of the commit
pub fn node_message(message: &str) -> GitCypher {
    GitCypher {
        var: "message".to_owned(),
        cypher: format!(
            "(message:Message {{message: \'{}\'}})",
            message.replace('\'', "\\\'")
        ),
    }
}

/// Creates a Cypher query to merge a node.
///
/// # Arguments
///
/// * `node` - The node to merge
pub fn merge_node(node: GitCypher) -> Query {
    let q = format!("MERGE {}", node.cypher);
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
pub fn merge_link(from: GitCypher, to: GitCypher, link: String) -> Query {
    let q = format!(
        "MATCH {}
    MATCH {}
    MERGE ({})-[:{}]->({})
    ",
        from.cypher, to.cypher, from.var, link, to.var
    );
    debug!("{}", q);
    query(q.as_str())
}

pub fn delete_reference(reference: GitCypher) -> Query {
    // MATCH (n:Reference {uri:'https://github.com/avrabe/meta-fmu.git', name:'dunfell'})
    // OPTIONAL MATCH (n)-[r]-()
    // WITH n,r LIMIT 50000
    // DELETE n,r
    // RETURN count(n) as deletedNodesCount

    let q = format!(
        "MATCH {} OPTIONAL MATCH ({})-[r]-() DELETE {}, r",
        reference.cypher, reference.var, reference.var
    );
    debug!("{}", q);
    query(q.as_str())
}

pub struct GraphDatabase {
    graph: Option<Arc<Graph>>,
}

impl GraphDatabase {
    pub async fn new(uri: String, user: String, password: String, db: String) -> GraphDatabase {
        let config = ConfigBuilder::new()
            .uri(uri)
            .user(user)
            .password(password)
            .db(db)
            .build()
            .unwrap();
        let graph = Graph::connect(config).await;
        match graph {
            Ok(graph) => GraphDatabase {
                graph: Some(Arc::new(graph)),
            },
            Err(err) => {
                error!("Error connecting to Graph database: {:?}", err);
                GraphDatabase { graph: None }
            }
        }
    }

    pub async fn txn_run_queries(&self, queries: Vec<Query>) -> Result<(), Box<dyn Error>> {
        match &self.graph {
            Some(graph) => {
                let txn = graph.start_txn().await?;
                txn.run_queries(queries).await.unwrap();

                txn.commit().await?;
            }
            None => error!("No graph connection"),
        }
        Ok(())
    }

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
    pub async fn query_branches_for_repository(&self, git_uri: &str) -> HashSet<String> {
        let mut res = HashSet::<String>::new();
        match &self.graph {
            Some(graph) => {
                let mut result = graph
                .execute(query("MATCH (h:Repository {uri: $uri})-[:has]->(r:Reference) return r").param("uri", git_uri))
                .await
                .unwrap();
                while let Ok(Some(row)) = result.next().await {
                    let node: Node = row.get("r").unwrap();
                    let name: String = node.get("name").unwrap();
                    res.insert(name.clone());
                    debug!("{}", name);
                }
            }
            None => error!("No graph connection"),
        }
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_reference() {
        let name = "foo";
        let uri = "http://example.com";

        let result = node_reference(name, uri);

        assert_eq!(result.var, "reference".to_owned());
        assert_eq!(
            result.cypher,
            format!("(reference:Reference {{name: '{}', uri: '{}'}})", name, uri)
        );
    }

    #[test]
    fn test_node_kas_manifest() {
        let path = "foo/bar.yml";
        let oid = "abcdef123456";

        let result = node_kas_manifest(path, oid);

        assert_eq!(result.var, "repository");
        assert_eq!(
            result.cypher,
            format!(
                "(repository:Manifest {{path: '{}', oid: '{}', type: 'kas'}})",
                path, oid
            )
        );
    }
}
