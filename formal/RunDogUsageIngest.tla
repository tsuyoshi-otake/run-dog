--------------------------- MODULE RunDogUsageIngest ---------------------------
EXTENDS Naturals, FiniteSets, Sequences, TLC

\* Reference model for JSONL ingest, checkpoint, rebuild, and generation.
\* This is not a proof of the Rust collector.  TLC PASS on this spec does
\* not imply the Windows adapter is correct.
\*
\* Fault is a mutation switch:
\*   0 none
\*   1 no_dedupe
\*   2 cursor_ahead
\*   3 incomplete_commit
\*   4 stale_overwrite
\*
\* The collector sub-model below uses file 1 as the healthy hot log and file
\* 2 as the path that may remain unreadable. Its retry state is runtime-only;
\* committed cursors and aggregates are the restart source of truth.

CONSTANTS MaxIds, Fault

ASSUME MaxIds \in Nat /\ MaxIds > 0
ASSUME Fault \in 0..4

Ids == 1..MaxIds
Files == {1, 2}
Months == {1, 2}
MaxLen == 2
MaxGen == 2
MaxRetries == 3
HealthyFile == 1
BlockedFile == 2
MaxCollectorCount == Cardinality(Files) * MaxLen

RecOK(r) ==
    /\ r.complete \in BOOLEAN
    /\ r.id \in (Ids \cup {0})
    /\ r.month \in Months
    /\ r.complete => r.id \in Ids
    /\ ~r.complete => r.id = 0

VARIABLES recs, cursor, accepted, contribution,
          month, generation, visibleGeneration, lastGoodGeneration,
          ckCursor, ckAccepted, ckGeneration,
          rebuild, migrated,
          catchUp, discovered, registered, retryAttempts, terminal,
          appended, collectorCursor, committedCollectorCursor,
          collectorMonth, collectorToday, committedCollectorMonth,
          committedCollectorToday, collectorDay

vars == <<recs, cursor, accepted, contribution,
          month, generation, visibleGeneration, lastGoodGeneration,
          ckCursor, ckAccepted, ckGeneration,
          rebuild, migrated>>

collectorVars == <<catchUp, discovered, registered, retryAttempts, terminal,
                    appended, collectorCursor, committedCollectorCursor,
                    collectorMonth, collectorToday, committedCollectorMonth,
                    committedCollectorToday, collectorDay>>

allVars == vars \o collectorVars

CollectorInit ==
    /\ catchUp = TRUE
    /\ discovered = {}
    /\ registered = {}
    /\ retryAttempts = [f \in Files |-> 0]
    /\ terminal = {}
    /\ appended = [f \in Files |-> 0]
    /\ collectorCursor = [f \in Files |-> 0]
    /\ committedCollectorCursor = [f \in Files |-> 0]
    /\ collectorMonth = 0
    /\ collectorToday = 0
    /\ committedCollectorMonth = 0
    /\ committedCollectorToday = 0
    /\ collectorDay = 1

HasIncomplete(f) ==
    /\ Len(recs[f]) > 0
    /\ ~recs[f][Len(recs[f])].complete

CursorOnComplete(f, pos) ==
    IF pos = 0 THEN TRUE
    ELSE IF pos \in DOMAIN recs[f] THEN recs[f][pos].complete
    ELSE FALSE

SafeRestore(f) ==
    IF ckCursor[f] > Len(recs[f]) THEN 0 ELSE ckCursor[f]

Init ==
    /\ recs = [f \in Files |-> <<>>]
    /\ cursor = [f \in Files |-> 0]
    /\ accepted = {}
    /\ contribution = [id \in Ids |-> 0]
    /\ month = 1
    /\ generation = 0
    /\ visibleGeneration = 0
    /\ lastGoodGeneration = 0
    /\ ckCursor = [f \in Files |-> 0]
    /\ ckAccepted = {}
    /\ ckGeneration = 0
    /\ rebuild = FALSE
    /\ migrated = FALSE
    /\ CollectorInit

AppendComplete(f, id) ==
    /\ Len(recs[f]) < MaxLen
    /\ ~HasIncomplete(f)
    /\ recs' = [recs EXCEPT ![f] = Append(@, [complete |-> TRUE, id |-> id, month |-> month])]
    /\ UNCHANGED <<cursor, accepted, contribution, month, generation,
                  visibleGeneration, lastGoodGeneration, ckCursor, ckAccepted,
                  ckGeneration, rebuild, migrated>>

Replay(id) ==
    /\ \E f \in Files:
        \E i \in DOMAIN recs[f]:
            recs[f][i].complete /\ recs[f][i].id = id
    /\ \E dest \in Files: AppendComplete(dest, id)

DuplicateAcrossFiles(id, dest) ==
    /\ \E src \in Files:
        /\ src # dest
        /\ \E i \in DOMAIN recs[src]:
            recs[src][i].complete /\ recs[src][i].id = id
    /\ AppendComplete(dest, id)

AppendPartial(f) ==
    /\ Len(recs[f]) < MaxLen
    /\ ~HasIncomplete(f)
    /\ recs' = [recs EXCEPT ![f] = Append(@, [complete |-> FALSE, id |-> 0, month |-> month])]
    /\ UNCHANGED <<cursor, accepted, contribution, month, generation,
                  visibleGeneration, lastGoodGeneration, ckCursor, ckAccepted,
                  ckGeneration, rebuild, migrated>>

CompletePartial(f, id) ==
    /\ HasIncomplete(f)
    /\ recs' = [recs EXCEPT ![f] = [@ EXCEPT ![Len(recs[f])] = [complete |-> TRUE, id |-> id, month |-> month]]]
    /\ UNCHANGED <<cursor, accepted, contribution, month, generation,
                  visibleGeneration, lastGoodGeneration, ckCursor, ckAccepted,
                  ckGeneration, rebuild, migrated>>

ReadNext(f) ==
    /\ cursor[f] < Len(recs[f])
    /\ LET rec == recs[f][cursor[f] + 1] IN
        IF ~rec.complete THEN
            /\ Fault \in {2, 3}
            /\ cursor' = [cursor EXCEPT ![f] = @ + 1]
            /\ UNCHANGED <<recs, accepted, contribution, month, generation,
                          visibleGeneration, lastGoodGeneration, ckCursor,
                          ckAccepted, ckGeneration, rebuild, migrated>>
        ELSE
            /\ cursor' = [cursor EXCEPT ![f] = @ + 1]
            /\ IF rec.id \in accepted /\ Fault # 1 THEN
                UNCHANGED <<accepted, contribution>>
               ELSE
                /\ accepted' = accepted \cup {rec.id}
                /\ contribution' = [contribution EXCEPT ![rec.id] = @ + 1]
            /\ UNCHANGED <<recs, month, generation, visibleGeneration,
                          lastGoodGeneration, ckCursor, ckAccepted,
                          ckGeneration, rebuild, migrated>>

CommitCheckpoint ==
    /\ \/ Fault \in {2, 3}
       \/ \A f \in Files: CursorOnComplete(f, cursor[f])
    /\ ckCursor' = cursor
    /\ ckAccepted' = accepted
    /\ ckGeneration' = generation
    /\ UNCHANGED <<recs, cursor, accepted, contribution, month, generation,
                  visibleGeneration, lastGoodGeneration, rebuild, migrated>>

Crash ==
    /\ cursor # ckCursor \/ accepted # ckAccepted
    /\ cursor' = [f \in Files |-> SafeRestore(f)]
    /\ ckCursor' = [f \in Files |-> SafeRestore(f)]
    /\ accepted' = ckAccepted
    /\ contribution' = [id \in Ids |-> IF id \in ckAccepted THEN 1 ELSE 0]
    /\ rebuild' = FALSE
    /\ generation' = lastGoodGeneration
    /\ ckGeneration' = IF ckGeneration > lastGoodGeneration THEN lastGoodGeneration ELSE ckGeneration
    /\ UNCHANGED <<recs, month, visibleGeneration, lastGoodGeneration,
                  ckAccepted, migrated>>

RestartFromCheckpoint == Crash

Truncate(f) ==
    /\ Len(recs[f]) > 0
    /\ LET next == SubSeq(recs[f], 1, Len(recs[f]) - 1)
           newCursor == [cursor EXCEPT ![f] = IF cursor[f] > Len(next) THEN 0 ELSE cursor[f]]
           newCk == [ckCursor EXCEPT ![f] = IF ckCursor[f] > Len(next) THEN 0 ELSE ckCursor[f]]
       IN
        /\ recs' = [recs EXCEPT ![f] = next]
        /\ cursor' = newCursor
        /\ ckCursor' = newCk
        /\ IF newCursor = newCk THEN
             /\ accepted' = ckAccepted
             /\ contribution' = [id \in Ids |-> IF id \in ckAccepted THEN 1 ELSE 0]
           ELSE
             UNCHANGED <<accepted, contribution>>
    /\ UNCHANGED <<month, generation, visibleGeneration, lastGoodGeneration,
                  ckAccepted, ckGeneration, rebuild, migrated>>

Replace(f) ==
    /\ recs[f] # <<>>
    /\ LET newCursor == [cursor EXCEPT ![f] = 0]
           newCk == [ckCursor EXCEPT ![f] = 0]
       IN
        /\ recs' = [recs EXCEPT ![f] = <<>>]
        /\ cursor' = newCursor
        /\ ckCursor' = newCk
        /\ IF newCursor = newCk THEN
             /\ accepted' = ckAccepted
             /\ contribution' = [id \in Ids |-> IF id \in ckAccepted THEN 1 ELSE 0]
           ELSE
             UNCHANGED <<accepted, contribution>>
    /\ UNCHANGED <<month, generation, visibleGeneration, lastGoodGeneration,
                  ckAccepted, ckGeneration, rebuild, migrated>>

MonthRollover ==
    /\ month < 2
    /\ month' = month + 1
    /\ UNCHANGED <<recs, cursor, accepted, contribution, generation,
                  visibleGeneration, lastGoodGeneration, ckCursor, ckAccepted,
                  ckGeneration, rebuild, migrated>>

BeginRebuild ==
    /\ ~rebuild
    /\ generation < MaxGen
    /\ rebuild' = TRUE
    /\ generation' = generation + 1
    /\ UNCHANGED <<recs, cursor, accepted, contribution, month,
                  visibleGeneration, lastGoodGeneration, ckCursor, ckAccepted,
                  ckGeneration, migrated>>

CommitRebuild ==
    /\ rebuild
    /\ \A f \in Files: ~HasIncomplete(f)
    /\ visibleGeneration' = generation
    /\ lastGoodGeneration' = generation
    /\ rebuild' = FALSE
    /\ UNCHANGED <<recs, cursor, accepted, contribution, month, generation,
                  ckCursor, ckAccepted, ckGeneration, migrated>>

Migrate ==
    /\ migrated' = TRUE
    /\ UNCHANGED <<recs, cursor, accepted, contribution, month, generation,
                  visibleGeneration, lastGoodGeneration, ckCursor, ckAccepted,
                  ckGeneration, rebuild>>

GenerationSwap ==
    /\ ~rebuild
    /\ \A f \in Files: ~HasIncomplete(f)
    /\ visibleGeneration' = generation
    /\ lastGoodGeneration' = generation
    /\ UNCHANGED <<recs, cursor, accepted, contribution, month, generation,
                  ckCursor, ckAccepted, ckGeneration, rebuild, migrated>>

StaleOverwrite ==
    /\ Fault = 4
    /\ lastGoodGeneration > 0
    /\ visibleGeneration' = lastGoodGeneration - 1
    /\ UNCHANGED <<recs, cursor, accepted, contribution, month, generation,
                  lastGoodGeneration, ckCursor, ckAccepted, ckGeneration,
                  rebuild, migrated>>

CollectorDiscover(f) ==
    /\ f \in Files
    /\ f \notin discovered
    /\ discovered' = discovered \cup {f}
    /\ UNCHANGED <<catchUp, registered, retryAttempts, terminal, appended,
                    collectorCursor, committedCollectorCursor, collectorMonth,
                    collectorToday, committedCollectorMonth,
                    committedCollectorToday, collectorDay>>
    /\ UNCHANGED vars

CollectorRegister(f) ==
    /\ f \in Files
    /\ f = HealthyFile
    /\ f \in discovered
    /\ f \notin terminal
    /\ registered' = registered \cup {f}
    /\ UNCHANGED <<catchUp, discovered, retryAttempts, terminal, appended,
                    collectorCursor, committedCollectorCursor, collectorMonth,
                    collectorToday, committedCollectorMonth,
                    committedCollectorToday, collectorDay>>
    /\ UNCHANGED vars

CollectorPermanentFailure(f) ==
    /\ f \in Files
    /\ f \in discovered
    /\ f \notin registered
    /\ f \notin terminal
    /\ retryAttempts[f] < MaxRetries
    /\ retryAttempts' = [retryAttempts EXCEPT ![f] = @ + 1]
    /\ terminal' =
          IF retryAttempts[f] + 1 = MaxRetries
          THEN terminal \cup {f}
          ELSE terminal
    /\ UNCHANGED <<catchUp, discovered, registered, appended,
                    collectorCursor, committedCollectorCursor, collectorMonth,
                    collectorToday, committedCollectorMonth,
                    committedCollectorToday, collectorDay>>
    /\ UNCHANGED vars

CollectorAppend(f) ==
    /\ f \in Files
    /\ f \in registered
    /\ appended[f] < MaxLen
    /\ appended' = [appended EXCEPT ![f] = @ + 1]
    /\ UNCHANGED <<catchUp, discovered, registered, retryAttempts, terminal,
                    collectorCursor, committedCollectorCursor, collectorMonth,
                    collectorToday, committedCollectorMonth,
                    committedCollectorToday, collectorDay>>
    /\ UNCHANGED vars

CollectorScan(f) ==
    /\ f \in Files
    /\ f \in registered
    /\ collectorCursor[f] < appended[f]
    /\ LET delta == appended[f] - collectorCursor[f] IN
        /\ collectorCursor' = [collectorCursor EXCEPT ![f] = appended[f]]
        /\ collectorMonth' = collectorMonth + delta
        /\ collectorToday' = collectorToday + delta
    /\ UNCHANGED <<catchUp, discovered, registered, retryAttempts, terminal,
                    appended, committedCollectorCursor,
                    committedCollectorMonth, committedCollectorToday,
                    collectorDay>>
    /\ UNCHANGED vars

CollectorCommit ==
    /\ committedCollectorCursor' = collectorCursor
    /\ committedCollectorMonth' = collectorMonth
    /\ committedCollectorToday' = collectorToday
    /\ UNCHANGED <<catchUp, discovered, registered, retryAttempts, terminal,
                    appended, collectorCursor, collectorMonth, collectorToday,
                    collectorDay>>
    /\ UNCHANGED vars

CollectorRollover ==
    /\ collectorDay = 1
    /\ collectorDay' = 2
    /\ collectorToday' = 0
    /\ committedCollectorToday' = 0
    /\ UNCHANGED <<catchUp, discovered, registered, retryAttempts, terminal,
                    appended, collectorCursor, committedCollectorCursor,
                    collectorMonth, committedCollectorMonth>>
    /\ UNCHANGED vars

CollectorFinish ==
    /\ catchUp
    /\ \A f \in Files:
          f \in terminal
          \/ (f \in registered /\ collectorCursor[f] = appended[f])
    /\ catchUp' = FALSE
    /\ UNCHANGED <<discovered, registered, retryAttempts, terminal, appended,
                    collectorCursor, committedCollectorCursor, collectorMonth,
                    collectorToday, committedCollectorMonth,
                    committedCollectorToday, collectorDay>>
    /\ UNCHANGED vars

CollectorRestart ==
    /\ ~catchUp
    /\ collectorCursor' = committedCollectorCursor
    /\ collectorMonth' = committedCollectorMonth
    /\ collectorToday' = committedCollectorToday
    /\ discovered' = {}
    /\ registered' = {}
    /\ retryAttempts' = [f \in Files |-> 0]
    /\ terminal' = {}
    /\ catchUp' = TRUE
    /\ UNCHANGED <<appended, committedCollectorCursor,
                    committedCollectorMonth, committedCollectorToday,
                    collectorDay>>
    /\ UNCHANGED vars

CollectorNext ==
    \/ \E f \in Files: CollectorDiscover(f)
    \/ \E f \in Files: CollectorRegister(f)
    \/ CollectorPermanentFailure(BlockedFile)
    \/ \E f \in Files: CollectorAppend(f)
    \/ \E f \in Files: CollectorScan(f)
    \/ CollectorCommit
    \/ CollectorRollover
    \/ CollectorFinish
    \/ CollectorRestart

Idle == UNCHANGED vars

LegacyNext ==
    \/ \E f \in Files, id \in Ids: AppendComplete(f, id)
    \/ \E id \in Ids: Replay(id)
    \/ \E id \in Ids, dest \in Files: DuplicateAcrossFiles(id, dest)
    \/ \E f \in Files: AppendPartial(f)
    \/ \E f \in Files, id \in Ids: CompletePartial(f, id)
    \/ \E f \in Files: ReadNext(f)
    \/ CommitCheckpoint
    \/ Crash
    \/ \E f \in Files: Truncate(f)
    \/ \E f \in Files: Replace(f)
    \/ MonthRollover
    \/ BeginRebuild
    \/ CommitRebuild
    \/ Migrate
    \/ GenerationSwap
    \/ StaleOverwrite
    \/ Idle

Next ==
    \/ (LegacyNext /\ UNCHANGED collectorVars)
    \/ CollectorNext

LegacyStep ==
    /\ LegacyNext
    /\ UNCHANGED collectorVars

CollectorTypeOK ==
    /\ catchUp \in BOOLEAN
    /\ discovered \subseteq Files
    /\ registered \subseteq Files
    /\ retryAttempts \in [Files -> 0..MaxRetries]
    /\ terminal \subseteq Files
    /\ terminal \cap registered = {}
    /\ appended \in [Files -> 0..MaxLen]
    /\ collectorCursor \in [Files -> 0..MaxLen]
    /\ committedCollectorCursor \in [Files -> 0..MaxLen]
    /\ collectorMonth \in 0..MaxCollectorCount
    /\ collectorToday \in 0..MaxCollectorCount
    /\ committedCollectorMonth \in 0..MaxCollectorCount
    /\ committedCollectorToday \in 0..MaxCollectorCount
    /\ collectorDay \in 1..2
    /\ collectorMonth = collectorCursor[1] + collectorCursor[2]
    /\ committedCollectorMonth =
          committedCollectorCursor[1] + committedCollectorCursor[2]
    /\ collectorToday <= collectorMonth
    /\ committedCollectorToday <= committedCollectorMonth

CollectorNoDoubleCount ==
    /\ collectorMonth = collectorCursor[1] + collectorCursor[2]
    /\ committedCollectorMonth =
          committedCollectorCursor[1] + committedCollectorCursor[2]

TypeOK ==
    /\ \A f \in Files:
        /\ \A i \in DOMAIN recs[f]: RecOK(recs[f][i])
        /\ Len(recs[f]) <= MaxLen
        /\ cursor[f] \in 0..Len(recs[f])
        /\ ckCursor[f] \in 0..MaxLen
    /\ accepted \subseteq Ids
    /\ contribution \in [Ids -> 0..4]
    /\ month \in Months
    /\ generation \in 0..MaxGen
    /\ visibleGeneration \in 0..MaxGen
    /\ lastGoodGeneration \in 0..MaxGen
    /\ ckAccepted \subseteq Ids
    /\ ckGeneration \in 0..MaxGen
    /\ rebuild \in BOOLEAN
    /\ migrated \in BOOLEAN
    /\ CollectorTypeOK

NoDoubleCount ==
    /\ \A id \in Ids: contribution[id] <= 1
    /\ CollectorNoDoubleCount

\* Fairness is weak and applies only to transitions that are continuously
\* enabled while a catch-up is active. A terminal registration failure is
\* reached after MaxRetries fair failure steps; healthy discovery/registration
\* and scanning are independent. Restart is intentionally not fair so the
\* model can still check that each restart restores the last commit.
CollectorFairness ==
    /\ WF_collectorVars(CollectorDiscover(HealthyFile))
    /\ WF_collectorVars(CollectorDiscover(BlockedFile))
    /\ WF_collectorVars(CollectorRegister(HealthyFile))
    /\ WF_collectorVars(CollectorPermanentFailure(BlockedFile))
    /\ WF_collectorVars(CollectorScan(HealthyFile))
    /\ WF_collectorVars(CollectorFinish)

CatchUpSpec ==
    /\ Init
    /\ [][CollectorNext]_collectorVars
    /\ CollectorFairness

NoLostHealthyProgress ==
    []((catchUp
        /\ HealthyFile \in registered
        /\ collectorCursor[HealthyFile] < appended[HealthyFile])
        => <>(collectorCursor[HealthyFile] = appended[HealthyFile]))

CatchUpHasTerminalOutcome ==
    [](catchUp => <>((~catchUp)
        /\ (\A f \in Files:
              f \in terminal
              \/ (f \in registered /\ collectorCursor[f] = appended[f]))))

HotLogEventuallyObserved ==
    []((collectorCursor[HealthyFile] < appended[HealthyFile])
        => <>(collectorCursor[HealthyFile] = appended[HealthyFile]))

RestartPreservesCommittedState ==
    [][CollectorRestart =>
        /\ collectorCursor' = committedCollectorCursor
        /\ collectorMonth' = committedCollectorMonth
        /\ collectorToday' = committedCollectorToday]_collectorVars

RolloverPreventsStaleTodayRestore ==
    [][CollectorRollover =>
        /\ collectorToday' = 0
        /\ committedCollectorToday' = 0]_collectorVars

NoSilentLoss ==
    \A f \in Files:
        \A i \in 1..cursor[f]:
            recs[f][i].complete => recs[f][i].id \in accepted

\* Crash restores the checkpoint pair. Live cursor may later sit on the
\* same offsets after a shrink/rebuild while accepted still holds ids
\* from a truncated suffix; committed ids are never dropped.
RestartEquivalence ==
    cursor = ckCursor => ckAccepted \subseteq accepted

ReplayInvariance ==
    \A id \in Ids: contribution[id] <= 1

CursorAndAggregateSameGeneration ==
    /\ ckGeneration <= generation
    /\ visibleGeneration <= generation
    /\ lastGoodGeneration <= generation
    /\ ~rebuild => (ckGeneration <= visibleGeneration \/ ckGeneration = generation)

NoIncompleteRecordCommit ==
    /\ \A f \in Files: CursorOnComplete(f, cursor[f])
    /\ \A f \in Files: CursorOnComplete(f, ckCursor[f]) \/ ckCursor[f] > Len(recs[f])

MonthRolloverDoesNotRewindCursor ==
    [][\A f \in Files: (month' # month) => (cursor'[f] >= cursor[f])]_vars

MigrationIdempotent ==
    [][migrated => migrated']_vars

OnlyCompleteGenerationVisible ==
    /\ visibleGeneration = lastGoodGeneration
    /\ visibleGeneration <= generation
    /\ rebuild => (visibleGeneration # generation \/ generation = lastGoodGeneration)

RebuildDoesNotDestroyLastGoodGeneration ==
    [][(rebuild' = TRUE /\ rebuild = FALSE) => lastGoodGeneration' = lastGoodGeneration]_vars

CheckpointIsPrefix ==
    \A f \in Files:
        \/ ckCursor[f] <= Len(recs[f])
        \/ recs[f] = <<>>

Spec ==
    /\ Init
    /\ [][LegacyStep]_allVars

=============================================================================
