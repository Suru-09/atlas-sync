# AtlasSync

- AtlasSync is an indexing & synchronization tool based on a JSON CRDT. It is a prototype built onto the design and implementation chapters from the thesis with the same name.

---

## Dependencies

- make sure that the your Rust version installed is >= 1.77.0


## How to run

- if you are the one starting the sharing:
  ```bash
  cargo run -- --watched-path="your_path"
  ```
- if you are connecting to another peer
  ```bash
  cargo run -- --watched-path="your_path" --peer-id="the_peer_you_connect_to"
  ```
