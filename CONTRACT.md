# Contract - badbatch

This contract is the executable spec for badbatch. Every obligation below is stated so it can fail. The full contract is the first build act of this repo and it extends the stub obligations written here.

## Stub obligations

BB-S1. Every stamped artifact carries the agent, the prompt hash, and the model that made it, and a stamp missing any of those fields is rejected rather than stored incomplete.

BB-S2. A recall walk from any stamped artifact lists every downstream artifact derived from it, and an artifact reachable through the stamp graph but missing from the walk is a defect the tests must catch.

BB-S3. Every artifact in a recall result is sorted into exactly one reversibility class, reversible, reversible with compensation, or irreversible, and an unclassified artifact in the result is a defect.

BB-S4. The recall walk reports only and changes nothing, and executing a recall is a separate act that never happens as a side effect of walking the graph.

BB-S5. Every fixture and example in this repo is synthetic and built for this repo, and no real trace or session data appears anywhere in the tree.

## Round 1 obligations

Round 1 builds stamping for the files in one repo plus the recall walk, over recorded files only. A run record is the recorded text one run left behind about its own work in one repo. A stamp is the document the tool prints for one run record. A current state is a recorded list of the digests the paths in the repo carry now. The tool reads the files it is handed and prints documents, and it writes nothing into any repo it describes. No decision model is called in this round, and stamping and the walk are deterministic rules with no judgment in them, because the decision points the roadmap names belong to recall execution planning in a later round.

A meaningful line is a line that is neither blank nor a comment, and a comment line is a line whose first character that is not a space is the hash character. A run record opens with a header of four meaningful lines in this order, naming the run, the agent, the prompt hash, and the model. After the header come file entries and artifact entries in any order. A file entry names one path and one action, created, modified, or deleted, and a created or modified entry is followed at once by a content block, which is the word content on its own line, then the recorded post-run content lines exactly as recorded, then the word end on its own line. Inside a content block every line is literal content, blank lines and comment lines included, and a content line that reads exactly end closes the block, so that one line can never appear inside recorded content. A deleted entry carries no content block. An artifact entry names one path, the paths it derives from written after the words derived-from as a comma separated list, and its reversibility class written after the word class, and it is followed at once by a content block in the same shape. A path is one token with no spaces, never absolute, and never carrying a parent reference. The values on the header lines are one token each.

The digest of recorded content is the SHA-256 digest, written as sixty-four lowercase hex characters, of the UTF-8 encoding of the content lines with one newline character after each line, so content with no lines digests the empty string. The reversibility classes are named reversible, reversible-with-compensation, and irreversible, and their sort order in a recall result is the order named here, reversible first and irreversible last. Where one artifact derives from a stamped file by more than one path, the chain the walk prints is a shortest path, and ties between shortest paths are broken by the lexicographically smallest sequence of paths.

The formats in brief are shown here once, and the obligations below state them as rules that can fail.

```
run run-0007
agent writer-3
prompt-hash b41d90
model synth-writer-2

file src/report.py created
content
print("report")
end

file src/legacy.py deleted

artifact build/report.html derived-from src/report.py class reversible
content
<html>report</html>
end
```

```
stamp run-0007
agent writer-3
prompt-hash b41d90
model synth-writer-2
file src/legacy.py deleted none
file src/report.py created DIGEST
artifact build/report.html reversible DIGEST derived-from src/report.py
```

```
recall src/report.py
artifact build/report.html reversible current chain src/report.py -> build/report.html
```

BB-1. A run record parses only when its first four meaningful lines are the header lines in the order run, agent, prompt-hash, and model, each naming exactly one value, and a record missing any of those four fields, naming one of them twice, or opening with any other meaningful line is refused rather than stamped incomplete.

BB-2. A malformed run record is refused with an error that names the source and the defect, and the refusal covers a meaningful line that matches no entry form, a file entry whose action is not created, modified, or deleted, a created or modified file entry with no content block after it, a deleted file entry carrying a content block, a content block that reaches the end of the record with no closing end line, a duplicate file path, a duplicate artifact path, an artifact path that equals a file path, a path that is absolute or carries a parent reference, an artifact entry with an empty derivation list, an artifact whose derivation names a path the record never declares as a file or as an artifact, an artifact that derives from itself directly or through a chain of artifacts, and an artifact entry with no class or with a class outside the three named classes, and no refusal ever surfaces as a traceback or as a partial stamp.

BB-3. The stamp for one run record prints the stamp line naming the run, then the agent, the prompt hash, and the model from the record header, then one file line per touched file sorted by path, naming the path, the action, and the digest of its recorded content, with the word none in place of a digest for a deleted file, then one artifact line per declared artifact sorted by path, naming the path, its class, the digest of its recorded content, and its derivation parents sorted by path, and the stamp declares no file, no artifact, and no derivation link the record does not declare.

BB-4. The content digest is computed by the rule stated above and by no other rule, and the same recorded content carried under two different paths produces the same digest in both stamp lines, and recorded content differing in any one line produces a different digest, and every digest printed in a stamp is sixty-four lowercase hex characters.

BB-5. A stamp document parses only when it carries the stamp line and the agent, prompt hash, and model lines and one well formed file or artifact line per entry, and a stamp missing any of the four provenance fields is rejected rather than walked incomplete, and a malformed stamp line, a duplicate path across file and artifact lines, an artifact line whose derivation names a path the stamp does not carry, and a file or artifact line whose digest is neither sixty-four lowercase hex characters nor, for a deleted file only, the word none are each refused with an error that names the source and the defect.

BB-6. A current state document parses only as file lines each naming one path and one digest of sixty-four lowercase hex characters, and a duplicate path, a line that matches no form, or a digest of any other shape is refused with an error that names the source and the defect, and a current state may carry paths the stamp does not name and may omit paths the stamp names, because both are facts the walk reports rather than defects in the state.

BB-7. The recall walk starts from one stamped file named on the command line, and a start name that is not a file entry in the stamp is refused with an error that names the start and the defect, and the walk lists every artifact reachable from that file through derivation links, following links through artifacts as well as from the file itself, and an artifact reachable through the stamp graph but missing from the walk is a defect, and the walk lists artifacts only and never lists the start file itself.

BB-8. Every artifact in a recall result appears exactly once and carries exactly one reversibility class, the class its stamp line records, and the result is sorted first by class in the order reversible, reversible-with-compensation, and irreversible, and then by path within one class, and an artifact printed with no class or with a class its stamp line does not record is a defect.

BB-9. Every artifact line in a recall result names the artifact, its reversibility class, its status, and the chain of derivation from the start file, written as the start file and each artifact on the path in order, separated by the arrow marker, and the chain is a shortest derivation path from the start file to that artifact, and where two shortest paths exist the chain printed is the one whose sequence of paths is lexicographically smallest, and a chain naming a path that is not on a real derivation path from the start file is a defect.

BB-10. The status of an artifact in a recall result is current when the current state carries the artifact with the digest its stamp line records, drifted when the current state carries the artifact with a different digest, and missing when the current state does not carry the artifact at all, and the walk reports the status it finds and never repairs a drifted artifact, never restores a missing one, and never executes a recall as a side effect of walking.

BB-11. The stamp command and the recall command write only to the standard output and never modify the run record, the stamp, the current state, or any file in the repo they describe, and the standard output of each command is the document it was asked for and nothing else.

BB-12. Running the stamp command twice over the same run record produces byte identical stamps, and running the recall command twice over the same stamp, current state, and start file produces byte identical results, and neither output depends on the order of file entries, artifact entries, derivation parents, or current state lines in the input documents.

BB-13. The command line tool exits with code 0 when a stamp or a recall result is produced and with code 2 when any input is malformed or a file cannot be read, and a refusal prints one error line to the error stream and never a traceback, and a recall whose start file has no downstream artifacts prints the header line alone and still exits with code 0.

BB-14. The graph checks in this tool are iterative rather than recursive, so a run record whose derivation chain is thousands of artifacts deep parses and stamps in any entry order, and a derivation cycle at any depth is refused with the chain named, and no valid record ever surfaces a recursion failure.

BB-15. A line in every input format ends at a newline character and at nothing else, so a carriage return, a form feed, or any other control character inside a content line is content and is digested as recorded, and because the word that closes a content block must read exactly end with nothing after it, a record written with carriage return and newline line endings does not terminate its blocks and is refused as malformed rather than silently reinterpreted.
