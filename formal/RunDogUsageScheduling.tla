---------------------- MODULE RunDogUsageScheduling ----------------------
EXTENDS Naturals

\* Reference scheduler model, not a proof of the Rust collector. One
\* collector tick with a bounded old-file backlog. OldFiles=3 and
\* SearchPrefix=2 stand for a production queue longer than the former
\* 64-entry search. Two file-work slots are available, one current-day
\* event fits in one file read, cooldown has elapsed, and retries are due.
\*
\* queued: the current-day file is pending behind frontOld old files.
\* append: the current-day file was caught up, then received an append;
\*         it is the only due hot restat. An old pending read saturates its
\*         byte budget, so the append needs the separate reserved read.
\*
\* Fault 1 restores prefix-only pending search. Fault 2 removes the hot
\* read reservation. Component tests connect these abstractions to Rust.

CONSTANTS OldFiles, SearchPrefix, Fault
ASSUME OldFiles \in Nat /\ SearchPrefix \in Nat
ASSUME OldFiles > SearchPrefix /\ SearchPrefix > 0
ASSUME Fault \in 0..2

VARIABLES mode, frontOld, pendingToday, dirtyHot, observed,
          oldReads, ticks, readsLast
vars == <<mode, frontOld, pendingToday, dirtyHot, observed,
          oldReads, ticks, readsLast>>

Init ==
    /\ mode \in {"queued", "append"}
    /\ frontOld \in 0..OldFiles
    /\ (mode = "append" => frontOld = 0)
    /\ pendingToday = (mode = "queued")
    /\ dirtyHot = (mode = "append")
    /\ observed = FALSE
    /\ oldReads = 0
    /\ ticks = 0
    /\ readsLast = 0

QueuedRead == pendingToday /\ (Fault # 1 \/ frontOld < SearchPrefix)
HotRead == dirtyHot /\ Fault # 2

Tick ==
    /\ ticks < OldFiles + 2
    /\ frontOld' = IF (pendingToday /\ ~QueuedRead /\ frontOld > 0)
                    THEN frontOld - 1 ELSE frontOld
    /\ pendingToday' = (pendingToday /\ ~QueuedRead)
    /\ dirtyHot' = (dirtyHot /\ ~HotRead)
    /\ observed' = (observed \/ QueuedRead \/ HotRead)
    /\ oldReads' = oldReads + 1
    /\ readsLast' = IF (QueuedRead \/ HotRead) THEN 2 ELSE 1
    /\ ticks' = ticks + 1
    /\ UNCHANGED mode

Spec == Init /\ [][Tick]_vars /\ WF_vars(Tick)

TypeOK ==
    /\ mode \in {"queued", "append"}
    /\ frontOld \in 0..OldFiles
    /\ pendingToday \in BOOLEAN
    /\ dirtyHot \in BOOLEAN
    /\ observed \in BOOLEAN
    /\ oldReads \in 0..(OldFiles + 2)
    /\ ticks \in 0..(OldFiles + 2)
    /\ readsLast \in 0..2

\* Under the stated one-event/one-due-file bound, today's event and one
\* old file both advance on the first tick. This catches unbounded backlog
\* latency even though a finite old queue would eventually rotate through.
TodayWithinOneTick == ticks = 0 \/ observed
OldBacklogAdvances == oldReads = ticks
FileBudgetBound == readsLast <= 2
TodayEventuallyObserved == <>observed

=============================================================================
