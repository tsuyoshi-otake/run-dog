------------------------ MODULE RunDogCurrentProtocol ------------------------
EXTENDS Naturals, FiniteSets, TLC

\* Bounded adversarial model of the current external protocol.  It is not a
\* transliteration of Rust: it models the observed Registry contract -- one
\* Run write followed by three independent settings writes, no durable version
\* or operation ID, and three independent reads on restart.

CONSTANTS Actors, MaxRetries, TimeBound, ResourceLimit, SameGeneration

Workers == 1..Actors
InitialSettings == <<0, 10, FALSE>>
Candidate(w) == <<w, 10 + w, w = 1>>
CandidateGeneration(w) == IF SameGeneration THEN 1 ELSE w
MaxGeneration == IF SameGeneration THEN 1 ELSE Actors
SettingsDomain == {InitialSettings} \cup {Candidate(w) : w \in Workers}
ThemeDomain == 0..Actors
FpsDomain == 10..(10 + Actors)

Phases == {"new", "read", "validated", "run", "key", "theme", "fps",
           "done", "failed", "timedout", "cancelled"}
Terminal == {"done", "failed", "timedout", "cancelled"}
FailureTerminal == {"failed", "timedout", "cancelled"}
ReaderPhases == {"before", "theme", "themeFps", "done"}

ModelParametersOK ==
    /\ Actors \in Nat
    /\ Actors > 0
    /\ MaxRetries \in Nat
    /\ TimeBound \in Nat
    /\ TimeBound > 0
    /\ ResourceLimit \in Nat
    /\ ResourceLimit > 0
    /\ ResourceLimit <= Actors
    /\ SameGeneration \in BOOLEAN

VARIABLES settings, fieldOwner, runEntry, runOwner,
          phase, readGeneration, attempts, commitCount, reportedGeneration,
          logicalTime, readerPhase, readerTheme, readerFps, readerStartup,
          deleted, deletedEver, lateMutation

vars == <<settings, fieldOwner, runEntry, runOwner,
          phase, readGeneration, attempts, commitCount, reportedGeneration,
          logicalTime, readerPhase, readerTheme, readerFps, readerStartup,
          deleted, deletedEver, lateMutation>>

VisibleSettings == IF deleted THEN InitialSettings ELSE settings
ActiveWorkers == {w \in Workers : phase[w] \in {"run", "key", "theme", "fps"}}
ActiveCount == Cardinality(ActiveWorkers)
Max(x, y) == IF x > y THEN x ELSE y

Init ==
    /\ ModelParametersOK
    /\ settings = InitialSettings
    /\ fieldOwner = <<0, 0, 0>>
    /\ runEntry = FALSE
    /\ runOwner = 0
    /\ phase = [w \in Workers |-> "new"]
    /\ readGeneration = [w \in Workers |-> 0]
    /\ attempts = [w \in Workers |-> 0]
    /\ commitCount = [w \in Workers |-> 0]
    /\ reportedGeneration = 0
    /\ logicalTime = 0
    /\ readerPhase = "before"
    /\ readerTheme = InitialSettings[1]
    /\ readerFps = InitialSettings[2]
    /\ readerStartup = InitialSettings[3]
    /\ deleted = FALSE
    /\ deletedEver = FALSE
    /\ lateMutation = FALSE

Read(w) ==
    /\ phase[w] = "new"
    /\ phase' = [phase EXCEPT ![w] = "read"]
    /\ readGeneration' = [readGeneration EXCEPT ![w] = reportedGeneration]
    /\ UNCHANGED <<settings, fieldOwner, runEntry, runOwner, attempts,
                  commitCount, reportedGeneration, logicalTime, readerPhase,
                  readerTheme, readerFps, readerStartup, deleted, deletedEver,
                  lateMutation>>

Validate(w) ==
    /\ phase[w] = "read"
    /\ phase' = [phase EXCEPT ![w] = "validated"]
    /\ UNCHANGED <<settings, fieldOwner, runEntry, runOwner, readGeneration,
                  attempts, commitCount, reportedGeneration, logicalTime,
                  readerPhase, readerTheme, readerFps, readerStartup, deleted,
                  deletedEver, lateMutation>>

ValidationFails(w) ==
    /\ phase[w] = "read"
    /\ phase' = [phase EXCEPT ![w] = "failed"]
    /\ UNCHANGED <<settings, fieldOwner, runEntry, runOwner, readGeneration,
                  attempts, commitCount, reportedGeneration, logicalTime,
                  readerPhase, readerTheme, readerFps, readerStartup, deleted,
                  deletedEver, lateMutation>>

WriteRunEntry(w) ==
    /\ phase[w] = "validated"
    /\ phase' = [phase EXCEPT ![w] = "run"]
    /\ runEntry' = Candidate(w)[3]
    /\ runOwner' = w
    /\ UNCHANGED <<settings, fieldOwner, readGeneration, attempts, commitCount,
                  reportedGeneration, logicalTime, readerPhase, readerTheme,
                  readerFps, readerStartup, deleted, deletedEver, lateMutation>>

RunEntryFails(w) ==
    /\ phase[w] = "validated"
    /\ phase' = [phase EXCEPT ![w] = "failed"]
    /\ UNCHANGED <<settings, fieldOwner, runEntry, runOwner, readGeneration,
                  attempts, commitCount, reportedGeneration, logicalTime,
                  readerPhase, readerTheme, readerFps, readerStartup, deleted,
                  deletedEver, lateMutation>>

OpenSettingsKey(w) ==
    /\ phase[w] = "run"
    /\ ActiveCount <= ResourceLimit
    /\ phase' = [phase EXCEPT ![w] = "key"]
    /\ IF deleted
          THEN /\ settings' = InitialSettings
               /\ fieldOwner' = <<0, 0, 0>>
               /\ deleted' = FALSE
          ELSE /\ UNCHANGED <<settings, fieldOwner, deleted>>
    /\ UNCHANGED <<runEntry, runOwner, readGeneration, attempts, commitCount,
                  reportedGeneration, logicalTime, readerPhase, readerTheme,
                  readerFps, readerStartup, deletedEver, lateMutation>>

OpenSettingsKeyFails(w) ==
    /\ phase[w] = "run"
    /\ phase' = [phase EXCEPT ![w] = "failed"]
    /\ UNCHANGED <<settings, fieldOwner, runEntry, runOwner, readGeneration,
                  attempts, commitCount, reportedGeneration, logicalTime,
                  readerPhase, readerTheme, readerFps, readerStartup, deleted,
                  deletedEver, lateMutation>>

ResourceExhausted(w) ==
    /\ phase[w] = "run"
    /\ ActiveCount > ResourceLimit
    /\ phase' = [phase EXCEPT ![w] = "failed"]
    /\ UNCHANGED <<settings, fieldOwner, runEntry, runOwner, readGeneration,
                  attempts, commitCount, reportedGeneration, logicalTime,
                  readerPhase, readerTheme, readerFps, readerStartup, deleted,
                  deletedEver, lateMutation>>

WriteTheme(w) ==
    /\ phase[w] = "key"
    /\ phase' = [phase EXCEPT ![w] = "theme"]
    /\ settings' = [settings EXCEPT ![1] = Candidate(w)[1]]
    /\ fieldOwner' = [fieldOwner EXCEPT ![1] = w]
    /\ UNCHANGED <<runEntry, runOwner, readGeneration, attempts, commitCount,
                  reportedGeneration, logicalTime, readerPhase, readerTheme,
                  readerFps, readerStartup, deleted, deletedEver, lateMutation>>

\* save_settings ignores this error and continues to FpsLimit.
ThemeWriteFails(w) ==
    /\ phase[w] = "key"
    /\ phase' = [phase EXCEPT ![w] = "theme"]
    /\ UNCHANGED <<settings, fieldOwner, runEntry, runOwner, readGeneration,
                  attempts, commitCount, reportedGeneration, logicalTime,
                  readerPhase, readerTheme, readerFps, readerStartup, deleted,
                  deletedEver, lateMutation>>

WriteFps(w) ==
    /\ phase[w] = "theme"
    /\ phase' = [phase EXCEPT ![w] = "fps"]
    /\ settings' = [settings EXCEPT ![2] = Candidate(w)[2]]
    /\ fieldOwner' = [fieldOwner EXCEPT ![2] = w]
    /\ UNCHANGED <<runEntry, runOwner, readGeneration, attempts, commitCount,
                  reportedGeneration, logicalTime, readerPhase, readerTheme,
                  readerFps, readerStartup, deleted, deletedEver, lateMutation>>

\* This error is also ignored; the launcher may report success despite it.
FpsWriteFails(w) ==
    /\ phase[w] = "theme"
    /\ phase' = [phase EXCEPT ![w] = "fps"]
    /\ UNCHANGED <<settings, fieldOwner, runEntry, runOwner, readGeneration,
                  attempts, commitCount, reportedGeneration, logicalTime,
                  readerPhase, readerTheme, readerFps, readerStartup, deleted,
                  deletedEver, lateMutation>>

WriteStartupValue(w) ==
    /\ phase[w] = "fps"
    /\ phase' = [phase EXCEPT ![w] = "done"]
    /\ settings' = [settings EXCEPT ![3] = Candidate(w)[3]]
    /\ fieldOwner' = [fieldOwner EXCEPT ![3] = w]
    /\ commitCount' = [commitCount EXCEPT ![w] = @ + 1]
    /\ reportedGeneration' = Max(reportedGeneration, CandidateGeneration(w))
    /\ UNCHANGED <<runEntry, runOwner, readGeneration, attempts, logicalTime,
                  readerPhase, readerTheme, readerFps, readerStartup, deleted,
                  deletedEver, lateMutation>>

StartupValueFails(w) ==
    /\ phase[w] = "fps"
    /\ phase' = [phase EXCEPT ![w] = "done"]
    /\ UNCHANGED <<settings, fieldOwner, runEntry, runOwner, readGeneration,
                  attempts, commitCount, reportedGeneration, logicalTime,
                  readerPhase, readerTheme, readerFps, readerStartup, deleted,
                  deletedEver, lateMutation>>

Crash(w) ==
    /\ phase[w] \in {"run", "key", "theme", "fps"}
    /\ phase' = [phase EXCEPT ![w] = "failed"]
    /\ UNCHANGED <<settings, fieldOwner, runEntry, runOwner, readGeneration,
                  attempts, commitCount, reportedGeneration, logicalTime,
                  readerPhase, readerTheme, readerFps, readerStartup, deleted,
                  deletedEver, lateMutation>>

Timeout(w) ==
    /\ phase[w] \notin Terminal
    /\ logicalTime = TimeBound
    /\ phase' = [phase EXCEPT ![w] = "timedout"]
    /\ UNCHANGED <<settings, fieldOwner, runEntry, runOwner, readGeneration,
                  attempts, commitCount, reportedGeneration, logicalTime,
                  readerPhase, readerTheme, readerFps, readerStartup, deleted,
                  deletedEver, lateMutation>>

Cancel(w) ==
    /\ phase[w] \notin Terminal
    /\ phase' = [phase EXCEPT ![w] = "cancelled"]
    /\ UNCHANGED <<settings, fieldOwner, runEntry, runOwner, readGeneration,
                  attempts, commitCount, reportedGeneration, logicalTime,
                  readerPhase, readerTheme, readerFps, readerStartup, deleted,
                  deletedEver, lateMutation>>

\* Models an external completion that arrives after a caller deadline.  The
\* implementation currently has no durable operation ID to reject it.
LateSuccess(w) ==
    /\ phase[w] = "timedout"
    /\ runEntry' = Candidate(w)[3]
    /\ runOwner' = w
    /\ lateMutation' = TRUE
    /\ UNCHANGED <<settings, fieldOwner, phase, readGeneration, attempts,
                  commitCount, reportedGeneration, logicalTime, readerPhase,
                  readerTheme, readerFps, readerStartup, deleted, deletedEver>>

Retry(w) ==
    /\ phase[w] \in FailureTerminal
    /\ attempts[w] < MaxRetries
    /\ phase' = [phase EXCEPT ![w] = "new"]
    /\ attempts' = [attempts EXCEPT ![w] = @ + 1]
    /\ UNCHANGED <<settings, fieldOwner, runEntry, runOwner, readGeneration,
                  commitCount, reportedGeneration, logicalTime, readerPhase,
                  readerTheme, readerFps, readerStartup, deleted, deletedEver,
                  lateMutation>>

DuplicateDelivery(w) ==
    /\ phase[w] = "done"
    /\ attempts[w] < MaxRetries
    /\ phase' = [phase EXCEPT ![w] = "new"]
    /\ attempts' = [attempts EXCEPT ![w] = @ + 1]
    /\ UNCHANGED <<settings, fieldOwner, runEntry, runOwner, readGeneration,
                  commitCount, reportedGeneration, logicalTime, readerPhase,
                  readerTheme, readerFps, readerStartup, deleted, deletedEver,
                  lateMutation>>

DeleteSettingsKey ==
    /\ ~deletedEver
    /\ deleted' = TRUE
    /\ deletedEver' = TRUE
    /\ UNCHANGED <<settings, fieldOwner, runEntry, runOwner, phase,
                  readGeneration, attempts, commitCount, reportedGeneration,
                  logicalTime, readerPhase, readerTheme, readerFps,
                  readerStartup, lateMutation>>

ReadTheme ==
    /\ readerPhase = "before"
    /\ readerPhase' = "theme"
    /\ readerTheme' = VisibleSettings[1]
    /\ UNCHANGED <<settings, fieldOwner, runEntry, runOwner, phase,
                  readGeneration, attempts, commitCount, reportedGeneration,
                  logicalTime, readerFps, readerStartup, deleted, deletedEver,
                  lateMutation>>

ReadFps ==
    /\ readerPhase = "theme"
    /\ readerPhase' = "themeFps"
    /\ readerFps' = VisibleSettings[2]
    /\ UNCHANGED <<settings, fieldOwner, runEntry, runOwner, phase,
                  readGeneration, attempts, commitCount, reportedGeneration,
                  logicalTime, readerTheme, readerStartup, deleted, deletedEver,
                  lateMutation>>

ReadStartup ==
    /\ readerPhase = "themeFps"
    /\ readerPhase' = "done"
    /\ readerStartup' = VisibleSettings[3]
    /\ UNCHANGED <<settings, fieldOwner, runEntry, runOwner, phase,
                  readGeneration, attempts, commitCount, reportedGeneration,
                  logicalTime, readerTheme, readerFps, deleted, deletedEver,
                  lateMutation>>

AdvanceTime ==
    /\ logicalTime < TimeBound
    /\ logicalTime' = logicalTime + 1
    /\ UNCHANGED <<settings, fieldOwner, runEntry, runOwner, phase,
                  readGeneration, attempts, commitCount, reportedGeneration,
                  readerPhase, readerTheme, readerFps, readerStartup, deleted,
                  deletedEver, lateMutation>>

Idle == UNCHANGED vars

Next ==
    \/ \E w \in Workers : Read(w)
    \/ \E w \in Workers : Validate(w)
    \/ \E w \in Workers : ValidationFails(w)
    \/ \E w \in Workers : WriteRunEntry(w)
    \/ \E w \in Workers : RunEntryFails(w)
    \/ \E w \in Workers : OpenSettingsKey(w)
    \/ \E w \in Workers : OpenSettingsKeyFails(w)
    \/ \E w \in Workers : ResourceExhausted(w)
    \/ \E w \in Workers : WriteTheme(w)
    \/ \E w \in Workers : ThemeWriteFails(w)
    \/ \E w \in Workers : WriteFps(w)
    \/ \E w \in Workers : FpsWriteFails(w)
    \/ \E w \in Workers : WriteStartupValue(w)
    \/ \E w \in Workers : StartupValueFails(w)
    \/ \E w \in Workers : Crash(w)
    \/ \E w \in Workers : Timeout(w)
    \/ \E w \in Workers : Cancel(w)
    \/ \E w \in Workers : LateSuccess(w)
    \/ \E w \in Workers : Retry(w)
    \/ \E w \in Workers : DuplicateDelivery(w)
    \/ DeleteSettingsKey
    \/ ReadTheme
    \/ ReadFps
    \/ ReadStartup
    \/ AdvanceTime
    \/ Idle

TypeOK ==
    /\ settings[1] \in ThemeDomain
    /\ settings[2] \in FpsDomain
    /\ settings[3] \in BOOLEAN
    /\ fieldOwner \in [1..3 -> 0..Actors]
    /\ runEntry \in BOOLEAN
    /\ runOwner \in 0..Actors
    /\ phase \in [Workers -> Phases]
    /\ readGeneration \in [Workers -> 0..MaxGeneration]
    /\ attempts \in [Workers -> 0..MaxRetries]
    /\ commitCount \in [Workers -> 0..(MaxRetries + 1)]
    /\ reportedGeneration \in 0..MaxGeneration
    /\ logicalTime \in 0..TimeBound
    /\ readerPhase \in ReaderPhases
    /\ readerTheme \in ThemeDomain
    /\ readerFps \in FpsDomain
    /\ readerStartup \in BOOLEAN
    /\ deleted \in BOOLEAN
    /\ deletedEver \in BOOLEAN
    /\ lateMutation \in BOOLEAN

FieldAtomic == fieldOwner[1] = fieldOwner[2] /\ fieldOwner[2] = fieldOwner[3]
StartupAgreement == runEntry = VisibleSettings[3]
SnapshotConsistent ==
    readerPhase = "done" =>
        <<readerTheme, readerFps, readerStartup>> \in SettingsDomain
FailureSafety ==
    \A w \in Workers :
        phase[w] \in FailureTerminal =>
            /\ runOwner # w
            /\ fieldOwner[1] # w
            /\ fieldOwner[2] # w
            /\ fieldOwner[3] # w
NoLateMutation == ~lateMutation
AtMostOnceCommit == \A w \in Workers : commitCount[w] <= 1
EqualVersionSingleWinner ==
    SameGeneration =>
        \A a \in Workers : \A b \in Workers :
            a # b => ~(commitCount[a] > 0 /\ commitCount[b] > 0)
NoRecreateAfterDelete == deletedEver => deleted
NoStaleWriteAfterNewer ==
    \A old \in Workers : \A newer \in Workers :
        /\ CandidateGeneration(old) < CandidateGeneration(newer)
        /\ phase[newer] = "done"
        => /\ fieldOwner[1] # old
           /\ fieldOwner[2] # old
           /\ fieldOwner[3] # old
=============================================================================
