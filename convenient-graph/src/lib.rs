//! Generic directed acyclic graph (DAG) library for dependency resolution and caching.
//!
//! This crate provides a generic DAG implementation that can be used for:
//! - Recipe dependency graphs
//! - Task dependency graphs
//! - Include dependency graphs
//! - Package dependency graphs
//!
//! # Features
//!
//! - Generic nodes and edges with type parameters
//! - Topological sorting using Kahn's algorithm
//! - Cycle detection
//! - Dependency and dependent tracking
//! - Content-based hashing for cache invalidation
//! - Optional serde support
//!
//! # Example
//!
//! ```
//! use convenient_graph::{DAG, NodeId};
//!
//! // Create a DAG with string nodes and unit edges
//! let mut dag = DAG::<String, ()>::new();
//!
//! // Add nodes
//! let a = dag.add_node("a".to_string());
//! let b = dag.add_node("b".to_string());
//! let c = dag.add_node("c".to_string());
//!
//! // Add edges (a before b, b before c)
//! dag.add_edge(a, b, ()).unwrap(); // b depends on a
//! dag.add_edge(b, c, ()).unwrap(); // c depends on b
//!
//! // Get topological order
//! let order = dag.topological_sort().unwrap();
//! assert_eq!(order, vec![a, b, c]);
//! ```

#![warn(missing_docs)]
#![deny(unsafe_code)]
#![warn(unused_results)]

use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::hash::{Hash, Hasher};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Node identifier in the DAG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NodeId(usize);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Node({})", self.0)
    }
}

/// Error types for DAG operations.
#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    /// Cycle detected in the graph
    #[error("Cycle detected in graph: {0}")]
    CycleDetected(String),

    /// Node not found
    #[error("Node {0} not found in graph")]
    NodeNotFound(NodeId),

    /// Invalid operation
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
}

/// Result type for DAG operations.
pub type GraphResult<T> = Result<T, GraphError>;

/// A node in the DAG containing data and tracking its edges.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
struct Node<N> {
    id: NodeId,
    data: N,
    // Outgoing edges (this node -> other nodes)
    outgoing: HashSet<NodeId>,
    // Incoming edges (other nodes -> this node)
    incoming: HashSet<NodeId>,
}

/// An edge in the DAG connecting two nodes with optional data.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
struct Edge<E> {
    from: NodeId,
    to: NodeId,
    data: E,
}

/// Generic directed acyclic graph (DAG).
///
/// A DAG is a directed graph with no cycles. This implementation provides:
/// - O(1) node lookup
/// - O(1) edge addition (with cycle check)
/// - O(V + E) topological sort
/// - O(V + E) cycle detection
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DAG<N, E> {
    nodes: HashMap<NodeId, Node<N>>,
    edges: Vec<Edge<E>>,
    next_id: usize,
}

impl<N, E> Default for DAG<N, E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<N, E> DAG<N, E> {
    /// Create a new empty DAG.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
            next_id: 0,
        }
    }

    /// Add a node to the graph and return its ID.
    pub fn add_node(&mut self, data: N) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;

        let node = Node {
            id,
            data,
            outgoing: HashSet::new(),
            incoming: HashSet::new(),
        };

        let _ = self.nodes.insert(id, node);
        id
    }

    /// Add a directed edge from `from` to `to` with associated data.
    ///
    /// The edge represents precedence: `from` must be processed before `to`.
    /// For example, if task B depends on task A, call `add_edge(A, B, ...)`.
    ///
    /// Returns an error if adding the edge would create a cycle.
    ///
    /// # Errors
    ///
    /// - `GraphError::NodeNotFound` if either node doesn't exist
    /// - `GraphError::CycleDetected` if adding the edge would create a cycle
    pub fn add_edge(&mut self, from: NodeId, to: NodeId, data: E) -> GraphResult<()> {
        // Check nodes exist
        if !self.nodes.contains_key(&from) {
            return Err(GraphError::NodeNotFound(from));
        }
        if !self.nodes.contains_key(&to) {
            return Err(GraphError::NodeNotFound(to));
        }

        // Check if edge would create cycle
        if self.would_create_cycle(from, to) {
            return Err(GraphError::CycleDetected(format!(
                "Adding edge {} -> {} would create a cycle",
                from, to
            )));
        }

        // Add edge
        let edge = Edge { from, to, data };
        self.edges.push(edge);

        // Update node connections
        // from has an outgoing edge to to (from -> to)
        // to has from as a dependency (incoming edge from from)
        if let Some(from_node) = self.nodes.get_mut(&from) {
            let _ = from_node.outgoing.insert(to);
        }
        if let Some(to_node) = self.nodes.get_mut(&to) {
            let _ = to_node.incoming.insert(from);
        }

        Ok(())
    }

    /// Check if adding an edge from `from` to `to` would create a cycle.
    ///
    /// Uses DFS to detect if there's already a path from `to` to `from`.
    fn would_create_cycle(&self, from: NodeId, to: NodeId) -> bool {
        // If to can reach from, adding from->to creates a cycle
        self.can_reach(to, from)
    }

    /// Check if there's a path from `start` to `end`.
    fn can_reach(&self, start: NodeId, end: NodeId) -> bool {
        if start == end {
            return true;
        }

        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(start);

        while let Some(current) = queue.pop_front() {
            if current == end {
                return true;
            }

            if visited.contains(&current) {
                continue;
            }
            let _ = visited.insert(current);

            if let Some(node) = self.nodes.get(&current) {
                for &neighbor in &node.outgoing {
                    if !visited.contains(&neighbor) {
                        queue.push_back(neighbor);
                    }
                }
            }
        }

        false
    }

    /// Get a reference to a node's data.
    ///
    /// # Errors
    ///
    /// Returns `GraphError::NodeNotFound` if the node doesn't exist.
    pub fn node(&self, id: NodeId) -> GraphResult<&N> {
        self.nodes
            .get(&id)
            .map(|node| &node.data)
            .ok_or(GraphError::NodeNotFound(id))
    }

    /// Get a mutable reference to a node's data.
    ///
    /// # Errors
    ///
    /// Returns `GraphError::NodeNotFound` if the node doesn't exist.
    pub fn node_mut(&mut self, id: NodeId) -> GraphResult<&mut N> {
        self.nodes
            .get_mut(&id)
            .map(|node| &mut node.data)
            .ok_or(GraphError::NodeNotFound(id))
    }

    /// Get all node IDs in the graph.
    #[must_use]
    pub fn node_ids(&self) -> Vec<NodeId> {
        self.nodes.keys().copied().collect()
    }

    /// Get the number of nodes in the graph.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get the number of edges in the graph.
    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Get all direct dependencies (incoming edges) of a node.
    ///
    /// # Errors
    ///
    /// Returns `GraphError::NodeNotFound` if the node doesn't exist.
    pub fn dependencies(&self, id: NodeId) -> GraphResult<Vec<NodeId>> {
        self.nodes
            .get(&id)
            .map(|node| node.incoming.iter().copied().collect())
            .ok_or(GraphError::NodeNotFound(id))
    }

    /// Get all direct dependents (outgoing edges) of a node.
    ///
    /// # Errors
    ///
    /// Returns `GraphError::NodeNotFound` if the node doesn't exist.
    pub fn dependents(&self, id: NodeId) -> GraphResult<Vec<NodeId>> {
        self.nodes
            .get(&id)
            .map(|node| node.outgoing.iter().copied().collect())
            .ok_or(GraphError::NodeNotFound(id))
    }

    /// Perform topological sort using Kahn's algorithm.
    ///
    /// Returns nodes in dependency order (dependencies before dependents).
    ///
    /// # Errors
    ///
    /// Returns `GraphError::CycleDetected` if the graph contains a cycle.
    pub fn topological_sort(&self) -> GraphResult<Vec<NodeId>> {
        // Calculate in-degree for each node
        let mut in_degree: HashMap<NodeId, usize> = HashMap::new();
        for id in self.nodes.keys() {
            let _ = in_degree.insert(*id, 0);
        }
        for edge in &self.edges {
            *in_degree.entry(edge.to).or_insert(0) += 1;
        }

        // Queue of nodes with no incoming edges
        let mut queue: VecDeque<NodeId> = in_degree
            .iter()
            .filter(|&(_, &degree)| degree == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut result = Vec::new();

        while let Some(node_id) = queue.pop_front() {
            result.push(node_id);

            // Process all outgoing edges
            if let Some(node) = self.nodes.get(&node_id) {
                for &neighbor in &node.outgoing {
                    if let Some(degree) = in_degree.get_mut(&neighbor) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push_back(neighbor);
                        }
                    }
                }
            }
        }

        // If we processed all nodes, there's no cycle
        if result.len() == self.nodes.len() {
            Ok(result)
        } else {
            Err(GraphError::CycleDetected(
                "Graph contains a cycle".to_string(),
            ))
        }
    }

    /// Find all cycles in the graph.
    ///
    /// Returns a vector of cycles, where each cycle is represented as a vector of node IDs.
    #[must_use]
    pub fn find_cycles(&self) -> Vec<Vec<NodeId>> {
        let mut cycles = Vec::new();
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();
        let mut path = Vec::new();

        for &node_id in self.nodes.keys() {
            if !visited.contains(&node_id) {
                self.find_cycles_dfs(
                    node_id,
                    &mut visited,
                    &mut rec_stack,
                    &mut path,
                    &mut cycles,
                );
            }
        }

        cycles
    }

    fn find_cycles_dfs(
        &self,
        node_id: NodeId,
        visited: &mut HashSet<NodeId>,
        rec_stack: &mut HashSet<NodeId>,
        path: &mut Vec<NodeId>,
        cycles: &mut Vec<Vec<NodeId>>,
    ) {
        let _ = visited.insert(node_id);
        let _ = rec_stack.insert(node_id);
        path.push(node_id);

        if let Some(node) = self.nodes.get(&node_id) {
            for &neighbor in &node.outgoing {
                if !visited.contains(&neighbor) {
                    self.find_cycles_dfs(neighbor, visited, rec_stack, path, cycles);
                } else if rec_stack.contains(&neighbor) {
                    // Found a cycle
                    if let Some(cycle_start) = path.iter().position(|&id| id == neighbor) {
                        cycles.push(path[cycle_start..].to_vec());
                    }
                }
            }
        }

        let _ = path.pop();
        let _ = rec_stack.remove(&node_id);
    }
}

impl<N, E> DAG<N, E>
where
    N: Hash,
{
    /// Calculate content hash for a node including all its dependencies.
    ///
    /// This can be used for cache invalidation - if the hash changes,
    /// the node or its dependencies have changed.
    ///
    /// # Errors
    ///
    /// Returns `GraphError::NodeNotFound` if the node doesn't exist.
    pub fn content_hash(&self, id: NodeId) -> GraphResult<String> {
        let mut hasher = Sha256::new();
        self.hash_node_recursive(id, &mut hasher, &mut HashSet::new())?;
        Ok(format!("{:x}", hasher.finalize()))
    }

    fn hash_node_recursive(
        &self,
        id: NodeId,
        hasher: &mut Sha256,
        visited: &mut HashSet<NodeId>,
    ) -> GraphResult<()> {
        if visited.contains(&id) {
            return Ok(());
        }
        let _ = visited.insert(id);

        let node = self
            .nodes
            .get(&id)
            .ok_or(GraphError::NodeNotFound(id))?;

        // Hash the node data
        let mut node_hasher = std::collections::hash_map::DefaultHasher::new();
        node.data.hash(&mut node_hasher);
        hasher.update(node_hasher.finish().to_le_bytes());

        // Hash dependencies recursively
        let mut deps: Vec<_> = node.incoming.iter().copied().collect();
        deps.sort_by_key(|id| id.0);
        for dep in deps {
            self.hash_node_recursive(dep, hasher, visited)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_empty_dag() {
        let dag = DAG::<String, ()>::new();
        assert_eq!(dag.node_count(), 0);
        assert_eq!(dag.edge_count(), 0);
    }

    #[test]
    fn test_add_nodes() {
        let mut dag = DAG::<String, ()>::new();
        let a = dag.add_node("a".to_string());
        let b = dag.add_node("b".to_string());
        let c = dag.add_node("c".to_string());

        assert_eq!(dag.node_count(), 3);
        assert_eq!(dag.node(a).unwrap(), "a");
        assert_eq!(dag.node(b).unwrap(), "b");
        assert_eq!(dag.node(c).unwrap(), "c");
    }

    #[test]
    fn test_add_edges() {
        let mut dag = DAG::<String, ()>::new();
        let a = dag.add_node("a".to_string());
        let b = dag.add_node("b".to_string());

        // a before b (a -> b)
        assert!(dag.add_edge(a, b, ()).is_ok());
        assert_eq!(dag.edge_count(), 1);
    }

    #[test]
    fn test_cycle_detection() {
        let mut dag = DAG::<String, ()>::new();
        let a = dag.add_node("a".to_string());
        let b = dag.add_node("b".to_string());
        let c = dag.add_node("c".to_string());

        // Create chain: a -> b -> c
        dag.add_edge(a, b, ()).unwrap();
        dag.add_edge(b, c, ()).unwrap();

        // Try to create cycle: c -> a
        let result = dag.add_edge(c, a, ());
        assert!(result.is_err());
        assert!(matches!(result, Err(GraphError::CycleDetected(_))));
    }

    #[test]
    fn test_topological_sort() {
        let mut dag = DAG::<String, ()>::new();
        let a = dag.add_node("a".to_string());
        let b = dag.add_node("b".to_string());
        let c = dag.add_node("c".to_string());

        dag.add_edge(a, b, ()).unwrap(); // a before b (b depends on a)
        dag.add_edge(b, c, ()).unwrap(); // b before c (c depends on b)

        let order = dag.topological_sort().unwrap();
        assert_eq!(order, vec![a, b, c]);
    }

    #[test]
    fn test_dependencies() {
        let mut dag = DAG::<String, ()>::new();
        let a = dag.add_node("a".to_string());
        let b = dag.add_node("b".to_string());
        let c = dag.add_node("c".to_string());

        // c depends on both a and b
        dag.add_edge(a, c, ()).unwrap(); // a before c
        dag.add_edge(b, c, ()).unwrap(); // b before c

        let deps = dag.dependencies(c).unwrap();
        assert_eq!(deps.len(), 2);
        assert!(deps.contains(&a));
        assert!(deps.contains(&b));
    }

    #[test]
    fn test_dependents() {
        let mut dag = DAG::<String, ()>::new();
        let a = dag.add_node("a".to_string());
        let b = dag.add_node("b".to_string());
        let c = dag.add_node("c".to_string());

        // Both b and c depend on a
        dag.add_edge(a, b, ()).unwrap(); // a before b
        dag.add_edge(a, c, ()).unwrap(); // a before c

        let deps = dag.dependents(a).unwrap();
        assert_eq!(deps.len(), 2);
        assert!(deps.contains(&b));
        assert!(deps.contains(&c));
    }

    #[test]
    fn test_content_hash() {
        let mut dag = DAG::<String, ()>::new();
        let a = dag.add_node("a".to_string());
        let b = dag.add_node("b".to_string());

        dag.add_edge(a, b, ()).unwrap(); // a before b (b depends on a)

        let hash1 = dag.content_hash(b).unwrap();

        // Hash should be consistent
        let hash2 = dag.content_hash(b).unwrap();
        assert_eq!(hash1, hash2);

        // Modify node data of dependency
        *dag.node_mut(a).unwrap() = "a_modified".to_string();

        // Hash should change since dependency changed
        let hash3 = dag.content_hash(b).unwrap();
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_find_cycles_no_cycle() {
        let mut dag = DAG::<String, ()>::new();
        let a = dag.add_node("a".to_string());
        let b = dag.add_node("b".to_string());
        dag.add_edge(a, b, ()).unwrap();

        let cycles = dag.find_cycles();
        assert_eq!(cycles.len(), 0);
    }

    #[test]
    fn test_complex_dag() {
        let mut dag = DAG::<&str, &str>::new();

        // Create a more complex DAG
        // Dependency tree: root -> {a, b} -> c -> d
        let root = dag.add_node("root");
        let a = dag.add_node("a");
        let b = dag.add_node("b");
        let c = dag.add_node("c");
        let d = dag.add_node("d");

        // Build the dependency chain
        dag.add_edge(root, a, "depends").unwrap(); // a depends on root
        dag.add_edge(root, b, "depends").unwrap(); // b depends on root
        dag.add_edge(a, c, "depends").unwrap(); // c depends on a
        dag.add_edge(b, c, "depends").unwrap(); // c depends on b
        dag.add_edge(c, d, "depends").unwrap(); // d depends on c

        let order = dag.topological_sort().unwrap();
        assert_eq!(order.len(), 5);

        // root should come before a and b
        let root_pos = order.iter().position(|&id| id == root).unwrap();
        let a_pos = order.iter().position(|&id| id == a).unwrap();
        let b_pos = order.iter().position(|&id| id == b).unwrap();
        assert!(root_pos < a_pos);
        assert!(root_pos < b_pos);

        // c should come after both a and b
        let c_pos = order.iter().position(|&id| id == c).unwrap();
        assert!(a_pos < c_pos);
        assert!(b_pos < c_pos);

        // d should come last
        let d_pos = order.iter().position(|&id| id == d).unwrap();
        assert!(c_pos < d_pos);
    }
}
