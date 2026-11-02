# Contract - badbatch

This contract is the executable spec for badbatch. Every obligation below is stated so it can fail. The full contract is the first build act of this repo and it extends the stub obligations written here.

## Stub obligations

BB-S1. Every stamped artifact carries the agent, the prompt hash, and the model that made it, and a stamp missing any of those fields is rejected rather than stored incomplete.

BB-S2. A recall walk from any stamped artifact lists every downstream artifact derived from it, and an artifact reachable through the stamp graph but missing from the walk is a defect the tests must catch.

BB-S3. Every artifact in a recall result is sorted into exactly one reversibility class, reversible, reversible with compensation, or irreversible, and an unclassified artifact in the result is a defect.

BB-S4. The recall walk reports only and changes nothing, and executing a recall is a separate act that never happens as a side effect of walking the graph.

BB-S5. Every fixture and example in this repo is synthetic and built for this repo, and no real trace or session data appears anywhere in the tree.
