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
./target/release/graph-git-cli -d
``````

In the neo4j explorer, search now for all repositories refered from the branch dunfell.
Return a list of all referenced repositories.

`MATCH (h:Repository {uri:'https://github.com/avrabe/meta-fmu.git'})-[:has]->(r:Reference {name:'dunfell'})<-[:links_to]-(c:Commit)-[:contains]->(m:Manifest)-[:refers]->(r1:Reference)<-[:links_to]-(c1:Commit)-[]->(p) return h,r,c,m,r1,c1,p`
![Example graph](./graph.svg)

## Development

```sh
cargo install --locked kani-verifier
cargo kani setup
```

## Neo4j helpers

Find all commits not linking to a branch.
`MATCH (a:Commit) WHERE not ((a)-[:links_to]->(:Reference)) RETURN a`

Create Indexes

```cypher
CREATE INDEX manifest IF NOT EXISTS FOR (n:Manifest) ON (n.type, n.path, n.oid)
CREATE INDEX commit IF NOT EXISTS FOR (n:Commit) ON (n.oid)
CREATE INDEX reference IF NOT EXISTS FOR (n:Reference) ON (n.name, n.uri)
CREATE INDEX tag IF NOT EXISTS FOR (n:Tag) ON (n.name, n.uri)

CREATE INDEX repository IF NOT EXISTS FOR (n:Repository) ON (n.uri)
CREATE INDEX person IF NOT EXISTS FOR (n:Person) ON (n.name, n.email)
CREATE INDEX message IF NOT EXISTS FOR (n:Message) ON (n.message)
```

Clear database
`MATCH (n) DETACH DELETE n`

Config the amount of visible data
`:config initialNodeDisplay: 1000`
`:config maxNeighbours: 300`

Good example to test: <https://android.googlesource.com/platform/manifest>

```sh
brew install --cask docker
brew install colima
colima start
export DOCKER_HOST=unix:///$HOME/.colima/docker.sock
```
