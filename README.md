# badbatch

A product recall for agent work, where every artifact is stamped at creation with the agent, prompt version, and model that made it, and one command walks that stamp graph to roll back everything a bad batch ever touched.

## What this is

Version control can revert commits and nothing else, and nobody tracks which agent, prompt, and model produced which change across files, tickets, messages, docs, and API calls, so a tainted batch cannot be recalled today at any price. Badbatch starts at the stamp, a provenance record written when work is produced, naming the agent, the prompt hash, the model version, and the parent artifacts it derived from. The recall is then a graph walk, not a search. Given a selector, it enumerates the full downstream set, sorts it into safely reversible, reversible with compensation, and irreversible, and executes the first two classes as one reviewed operation while filing the third as a damage report with owners. The boundary with its sibling is one sentence. Flighttape records the run, badbatch stamps what the run left behind.

## Status

Round 4 landed in PR 9 and is recorded closed here. Stamping, the recall walk, and the execution plan over recorded inputs are in place under contract obligations BB-1 to BB-29, carrying the agent, the prompt hash, and the model on every stamp, digesting recorded content, and sorting downstream artifacts by reversibility class with drift flagged and never repaired, and a trusted recorded judgment may now propose a class for an artifact the stamp leaves unclassed, printed with a proposed marker and never recorded as the class, and the plan now states the candidate compensations each artifact carries by class and status and may order them by a trusted recorded ranking while executing nothing, and the judgment validation now orders extra fields by their printed form everywhere it names one, with two readings of the stamp format and one of the fixture format recorded in the contract. Closed rounds 4 of 100. Part of the impatient programming portfolio, in the Fleet Operations arc.

## Licence

MIT. See LICENSE.
