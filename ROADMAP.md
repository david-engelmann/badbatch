# Roadmap - badbatch

Badbatch stamps what a run left behind and walks the stamp graph when a recall is needed. The setup slice is landed and the contract stub stands at BB-S1 to BB-S5. Round 1 builds stamping for one repo plus the recall walk that sorts downstream artifacts by reversibility class.

## Decision models in badbatch

Judgment enters at the walk's reporting edge, and nowhere else.

The recall walk itself is deterministic and stays that way. It follows the stamp graph, lists every downstream artifact, and sorts each into reversible, reversible with compensation, or irreversible from the stamp data. No model runs inside the traversal, and the walk changes nothing. Round 2 adopted the first decision point below: when the walk would list an artifact the stamp leaves unclassed, it may consult a judgment source for a proposed classification, and that consultation is the whole of the model's place in the walk.

Judgment enters later, when a recall moves from the walk to an execution plan and a compensation path is ambiguous. Some artifacts can be undone more than one way, and the compensations differ in what they restore and what they disturb. Ranking those candidate compensations for one artifact is a candidate for a decision model call shaped as a score for each candidate, rating how completely that compensation restores the prior state. Where the stamp data cannot bound the class at all, a choice call over the three reversibility classes is a candidate for a proposed classification.

What the model decides stays advisory in both places. A proposed classification is marked as proposed wherever the walk output is shown, and it never silently becomes the recorded class. A compensation ranking orders the options a human reviewer sees. Executing a recall remains a separate reviewed act that no model performs, approves, or triggers.

What stays deterministic is everything structural. Stamping at creation with the agent, the prompt hash, and the model is a rule with no judgment in it. The walk's completeness rule, an artifact reachable through the graph but missing from the walk is a defect, is a rule. The three reversibility classes and the report-only nature of the walk are rules. A model abstention or a confidence below the threshold set in the contract leaves the artifact unranked and unproposed, which is the state it was in before the model existed.

The candidate model is Typesafe's Jev, called through the program's Typesafe connector, with the OpenAI decision model as the comparison point. Both are evaluated against the same recorded fixtures before either is adopted at a decision point, and the deterministic core of this repo never depends on a provider being reachable.

Tests stay offline. Live answers are captured once, in a bounded recording session, as request and response fixture pairs built from synthetic stamp graphs, and the suite plays those fixtures back through a fixture player. No test calls the network. A schema test pins the fixture shape so a provider response that changes shape fails loudly in the recording session instead of silently in production.

No model seam was implemented in Round 1. Round 2 implements the proposed classification point as an offline fixture seam, and a trusted recorded judgment may propose a class for an artifact the stamp leaves unclassed, marked as proposed wherever the walk output is shown and never recorded as the class. Round 4 builds the execution plan surface the section above names and implements the compensation ranking point as an offline fixture seam beside the classification point, sharing the fixture envelope and the trust threshold, and a trusted recorded score may order the candidate compensations a reviewer sees while executing a recall remains a separate reviewed act the tool never performs.

The live-call budget for this repo is set as an envelope and owned by this arc. Trivial verification is capped at 5 calls. Fixture recording is capped at 150 calls in one designated session per round that needs new fixtures. A comparison evaluation is capped at 300 calls per model when a decision point is being adopted. Every call carries synthetic state only, every session is logged in the round record with its call count, and raising the envelope is a contract change recorded in this repo, never a silent overrun.
