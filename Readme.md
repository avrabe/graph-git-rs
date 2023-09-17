# graph-git-rs

[![Rust](https://github.com/avrabe/graph-git-rs/actions/workflows/rust.yml/badge.svg)](https://github.com/avrabe/graph-git-rs/actions/workflows/rust.yml)
[![codecov](https://codecov.io/gh/avrabe/graph-git-rs/graph/badge.svg?token=9rYlCv0G2W)](https://codecov.io/gh/avrabe/graph-git-rs)

A rust library for working with Git repositories as graphs.

## Example

Download and start the tool. Create a new database in neo4j. You can use the command line parameters to specify the database location.

```sh
git clone https://github.com/avrabe/graph-git-rs.git
cd graph-git-rs
cargo build --release
./target/release/graph-git-rs -d
``````

In the neo4j explorer, search now for all repositories refered from the branch dunfell.
Return a list of all referenced repositories.

`MATCH (h:Repository {uri:'https://github.com/avrabe/meta-fmu.git'})-[:has]->(r:Reference {name:'dunfell'})<-[:links_to]-(c:Commit)-[:contains]->(m:Manifest)-[:refers]->(r1:Reference)<-[:has]-(h1:Repository) return h,r,c,m,r1,h1`
![Example graph](./graph.svg)

## Neo4j helpers

Clear database
`MATCH (n) DETACH DELETE n`

Config the amount of visible data
`:config initialNodeDisplay: 1000`
`:config maxNeighbours: 300`

Good example to test: <https://android.googlesource.com/platform/manifest>
