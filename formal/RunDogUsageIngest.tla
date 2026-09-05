--------------------------- MODULE RunDogUsageIngest ---------------------------
EXTENDS Naturals, FiniteSets, Sequences, TLC

\* Reference model for JSONL ingest.  Not a proof of the Rust collector.
\* Producer may replay the same id.  The reader may only accept a complete
\* record and may contribute each id at most once.  The cursor never moves
\* past an incomplete tail.  Checkpoint restore replays a prior prefix.

CONSTANTS MaxIds

ASSUME MaxIds \in Nat /\ MaxIds > 0

Ids == 1..MaxIds
RecOK(r) ==
    /\ r.complete \in BOOLEAN
    /\ r.id \in (Ids \cup {0})
    /\ r.complete => r.id \in Ids
    /\ ~r.complete => r.id = 0

VARIABLES file, cursor, accepted, contribution,
          produced, checkpointCursor, checkpointAccepted

vars == <<file, cursor, accepted, contribution,
          produced, checkpointCursor, checkpointAccepted>>

Init ==
    /\ file = << >>
    /\ cursor = 0
    /\ accepted = {}
    /\ contribution = [id \in Ids |-> 0]
    /\ produced = {}
    /\ checkpointCursor = 0
    /\ checkpointAccepted = {}

HasIncomplete ==
    /\ Len(file) > 0
    /\ ~file[Len(file)].complete

NextComplete ==
    /\ cursor < Len(file)
    /\ file[cursor + 1].complete

ProducerComplete(id) ==
    /\ id \notin produced
    /\ ~HasIncomplete
    /\ file' = Append(file, [complete |-> TRUE, id |-> id])
    /\ produced' = produced \union {id}
    /\ UNCHANGED <<cursor, accepted, contribution,
                  checkpointCursor, checkpointAccepted>>

ProducerPartial ==
    /\ ~HasIncomplete
    /\ produced # Ids
    /\ file' = Append(file, [complete |-> FALSE, id |-> 0])
    /\ UNCHANGED <<cursor, accepted, contribution, produced,
                  checkpointCursor, checkpointAccepted>>

CompletePartial(id) ==
    /\ HasIncomplete
    /\ id \notin produced
    /\ file' = [file EXCEPT ![Len(file)] = [complete |-> TRUE, id |-> id]]
    /\ produced' = produced \union {id}
    /\ UNCHANGED <<cursor, accepted, contribution,
                  checkpointCursor, checkpointAccepted>>

ReadNext ==
    /\ NextComplete
    /\ LET rec == file[cursor + 1] IN
        /\ cursor' = cursor + 1
        /\ IF rec.id \in accepted
           THEN UNCHANGED <<accepted, contribution>>
           ELSE /\ accepted' = accepted \union {rec.id}
                /\ contribution' = [contribution EXCEPT ![rec.id] = @ + 1]
    /\ UNCHANGED <<file, produced, checkpointCursor, checkpointAccepted>>

CommitCheckpoint ==
    /\ checkpointCursor # cursor \/ checkpointAccepted # accepted
    /\ checkpointCursor' = cursor
    /\ checkpointAccepted' = accepted
    /\ UNCHANGED <<file, cursor, accepted, contribution, produced>>

RestartFromCheckpoint ==
    /\ cursor # checkpointCursor \/ accepted # checkpointAccepted
    /\ cursor' = checkpointCursor
    /\ accepted' = checkpointAccepted
    /\ contribution' = [id \in Ids |-> IF id \in checkpointAccepted THEN 1 ELSE 0]
    /\ UNCHANGED <<file, produced, checkpointCursor, checkpointAccepted>>

\* Explicit stutter.  Fairness below still governs producer/accepted progress.
Idle == UNCHANGED vars

Next ==
    \/ \E id \in Ids : ProducerComplete(id)
    \/ ProducerPartial
    \/ \E id \in Ids : CompletePartial(id)
    \/ ReadNext
    \/ CommitCheckpoint
    \/ RestartFromCheckpoint
    \/ Idle

TypeOK ==
    /\ \A i \in DOMAIN file : RecOK(file[i])
    /\ cursor \in 0..Len(file)
    /\ accepted \subseteq Ids
    /\ contribution \in [Ids -> 0..1]
    /\ produced \subseteq Ids
    /\ checkpointCursor \in 0..Len(file)
    /\ checkpointAccepted \subseteq Ids

AtMostOnceContribution ==
    \A id \in Ids : contribution[id] <= 1

NoPrematureCommit ==
    \A i \in DOMAIN file :
        ~file[i].complete => cursor < i

CheckpointConsistency ==
    /\ checkpointCursor <= Len(file)
    /\ \A i \in 1..checkpointCursor : file[i].complete
    /\ checkpointAccepted \subseteq
          {file[i].id : i \in {j \in 1..checkpointCursor : file[j].complete}}

AcceptedAgreesContribution ==
    \A id \in Ids : (id \in accepted) <=> (contribution[id] = 1)

ProducerProgress ==
    (produced # Ids) ~> (produced = Ids \/ HasIncomplete)

AcceptedProgress ==
    \A id \in Ids :
        (id \in produced /\ ~HasIncomplete) ~> (id \in accepted \/ cursor < Len(file))

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(ReadNext)
    /\ WF_vars(CommitCheckpoint)
    /\ \A id \in Ids :
          /\ WF_vars(ProducerComplete(id))
          /\ WF_vars(CompletePartial(id))
=============================================================================
