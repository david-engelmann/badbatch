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

## Round 2 obligations

Round 2 adopts the first of the two decision points the roadmap names, the proposed classification, and adopts it offline. Where the stamp data bounds no class for an artifact, a recorded judgment may propose one of the three reversibility classes for that artifact, and the proposal is marked as proposed wherever the walk output shows it and never becomes the recorded class. The second point the roadmap names, ranking candidate compensations when a recall moves from the walk to an execution plan, is not adopted in this round and remains future work for a round that builds an execution plan surface. No live model is called in this round and no test calls the network. Answers come from recorded fixture pairs played back through a fixture player, and every committed fixture is synthetic and built for this repo. The committed fixtures record a trusted proposal for each of the three classes, one abstention, and one answer at confidence 0.45, which falls below the trust threshold this round sets.

BB-16. Amendment to BB-2 and BB-5. The word unclassed may stand in the class position of an artifact entry in a run record and of an artifact line in a stamp, and it records that the stamp data bounds no class for that artifact. The word is not a fourth reversibility class, and the three classes and their order are unchanged. A class word that is neither one of the three named classes nor unclassed is refused exactly as BB-2 and BB-5 already refuse it, and an artifact entry that names no class at all is refused as BB-2 already refuses it. Stamping carries the marker through unchanged, so the stamp for a record with an unclassed artifact prints unclassed in the class position of that artifact line, and the stamp command asks for no judgment and the marker reaches the recall as recorded.

BB-17. A classification request is built for one unclassed artifact from the stamp and the walk and from nothing else. The request carries exactly four fields. The field artifact names the artifact path. The field parents names the derivation parents the stamp records for the artifact, sorted by path. The field digest names the digest the stamp line records for the artifact. The field status names the status the walk computes for the artifact against the current state under BB-10. The judgment point is named classification, and the labels a classification answer may choose among are exactly the three reversibility classes, reversible and reversible-with-compensation and irreversible.

BB-18. A fixture is one JSON file that pairs one classification request with one response and marks itself synthetic. A choice response carries exactly the fields kind and choice and confidence and probabilities, with kind naming choice, with choice naming one of the three classes, with confidence a number between 0 and 1, and with probabilities carrying a number between 0 and 1 for each of the three classes and no other label, the three numbers adding up to 1. An abstention response carries exactly the field kind, with kind naming abstention, and carries no choice and no score and no confidence. A fixture that breaks this shape in its envelope or its request or its response is refused when the player loads it, with a judgment error that names the file and the defect, and two fixtures that pair the same request are refused at load as an ambiguous recording. A test validates every committed fixture against this same shape, so a recorded answer that changes shape fails in the suite rather than in a recall.

BB-19. The fixture player answers a classification request only from a recorded fixture whose request matches the request exactly, compared as canonical JSON with sorted keys, and where no recorded fixture matches it returns no answer, and the player reads only the directory it is given and never opens a network connection. The walk checks an answer from any source against the recorded response shape for the classification point before it trusts the answer, and an answer outside that shape, carrying a field the shape does not carry or missing a field it does or naming a label outside the three classes, counts as no judgment at all.

BB-20. An answer counts as a judgment only when it is well formed for the classification point and its confidence reaches 0.6. An answer below that confidence and an abstention and a malformed answer and the absence of any judgment source are all treated the same way, as no judgment at all, and in every one of those cases the walk behaves exactly as it behaved before the seam existed.

BB-21. The recall command accepts an optional fixtures flag naming a fixtures directory, and a directory that cannot be loaded prints one error line and exits with code 2 and shows no traceback. The walk asks for a judgment only about an unclassed artifact it would list, once per such artifact, and an unclassed artifact the walk does not reach is never asked about and never refuses the walk. Where no trusted judgment proposes a class for an unclassed artifact the walk would list, the recall is refused with the same error line the stamp parser gives for a class word outside the three named classes, naming the source and the artifact, and where more than one such artifact lacks a trusted proposal the refusal names the first of them in path order, and the command exits with code 2 and prints that one line and no traceback. Where a trusted judgment proposes a class, the artifact appears in the result carrying the proposed class, printed as the class word followed by the word proposed, then its status and its chain as BB-9 and BB-10 state them, and it is sorted into the group of its proposed class under the order BB-8 states, amending BB-8 for this case alone. A proposed class is never written back into the stamp or into any file, and BB-11 stands over the seam unchanged. An artifact whose stamp line records a class is never asked about and is never re-proposed, and its result line is byte identical to the line the walk printed before the seam existed, and a walk over a stamp with no unclassed artifact it would list makes no requests at all and prints byte identical output with and without the flag.

BB-22. Amendment, added by the Round 2 review. Three precision fixes to BB-18 and BB-19. First, the three probabilities of a choice response add up to 1 within a tolerance of one thousandth, because decimal probability values rarely sum to exactly 1 in binary floating point arithmetic, and a response whose probabilities sum outside that band is refused at load and counts as no judgment from any source. Second, the fixture envelope carries exactly the fields synthetic, judgment_point, request, and response, and a fixture carrying any other top level field is refused at load naming the field. Third, a response kind that is not a string is a malformed answer, refused at load and counted as no judgment from any source, and a numeric field whose value cannot be converted to a finite float, however it was written in the JSON text, is not a number for the unit interval checks and is refused or counted as no judgment on the same terms.
