use neo4rs::{query, Query};
use tracing::debug;

/// Creates a Cypher node representation for a Git reference.
///
/// # Arguments
///
/// * `name` - The name of the reference
///
pub fn node_reference(name: &str) -> GitCypher {
    GitCypher {
        var: "reference".to_owned(),
        cypher: format!("(reference:Reference {{name: \'{}\'}})", name),
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

/// Creates a Cypher node representation for a Git person.
///
/// # Arguments
///
/// * `name` - The name of the person
pub fn node_person(name: &str, email: &str) -> GitCypher {
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
        cypher: format!("(message:Message {{message: \'{}\'}})", message),
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
