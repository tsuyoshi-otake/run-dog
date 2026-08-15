--------------------------- MODULE RunDogProtocol ---------------------------
EXTENDS Naturals, FiniteSets, TLC

\* Reference protocol, derived from the domain requirements rather than from
\* the Registry implementation.  A configuration is one generation containing
\* the three user settings and the Run entry.  A generation either commits as
\* a whole or remains invisible.
\*
\* Actors, retry budget, logical deadline, capacity, and equal-generation mode
\* are model constants.  `SameGeneration = TRUE` exercises the tie boundary.

CONSTANTS Actors, MaxRetries, TimeBound, ResourceLimit, SameGeneration

Workers == 1..Actors
InitialSettings == <<0, 10, FALSE>>
Desired(w) == <<w, 10 + w, w = 1>>
CandidateGeneration(w) == IF SameGeneration THEN 1 ELSE w
MaxGeneration == IF SameGeneration THEN 1 ELSE Actors
SettingsDomain == {InitialSettings} \cup {Desired(w) : w \in Workers}

Phases == {"new", "read", "validated", "begun", "done", "rejected",
           "failed", "timedout", "cancelled"}
Terminal == {"done", "rejected", "failed", "timedout", "cancelled"}
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

VARIABLES settings, runEntry, durableGeneration,
          phase, readGeneration, attempts, commitCount, logicalTime,
          snapshot, readerPhase, readerTheme, readerFps, readerStartup,
          lateIgnored

vars == <<settings, runEntry, durableGeneration,
          phase, readGeneration, attempts, commitCount, logicalTime,
          snapshot, readerPhase, readerTheme, readerFps, readerStartup,
          lateIgnored>>

ActiveWorkers == {w \in Workers : phase[w] = "begun"}
ActiveCount == Cardinality(ActiveWorkers)

Init ==
    /\ ModelParametersOK
    /\ settings = InitialSettings
    /\ runEntry = FALSE
    /\ durableGeneration = 0
    /\ phase = [w \in Workers |-> "new"]
    /\ readGeneration = [w \in Workers |-> 0]
    /\ attempts = [w \in Workers |-> 0]
    /\ commitCount = [w \in Workers |-> 0]
    /\ logicalTime = 0
    /\ snapshot = InitialSettings
    /\ readerPhase = "before"
    /\ readerTheme = InitialSettings[1]
    /\ readerFps = InitialSettings[2]
    /\ readerStartup = InitialSettings[3]
    /\ lateIgnored = 0

Read(w) ==
    /\ phase[w] = "new"
    /\ phase' = [phase EXCEPT ![w] = "read"]
    /\ readGeneration' = [readGeneration EXCEPT ![w] = durableGeneration]
    /\ UNCHANGED <<settings, runEntry, durableGeneration, attempts, commitCount,
                  logicalTime, snapshot, readerPhase, readerTheme, readerFps,
                  readerStartup, lateIgnored>>

Validate(w) ==
    /\ phase[w] = "read"
    /\ phase' = [phase EXCEPT ![w] = "validated"]
    /\ UNCHANGED <<settings, runEntry, durableGeneration, readGeneration,
                  attempts, commitCount, logicalTime, snapshot, readerPhase,
                  readerTheme, readerFps, readerStartup, lateIgnored>>

ValidationFails(w) ==
    /\ phase[w] = "read"
    /\ phase' = [phase EXCEPT ![w] = "failed"]
    /\ UNCHANGED <<settings, runEntry, durableGeneration, readGeneration,
                  attempts, commitCount, logicalTime, snapshot, readerPhase,
                  readerTheme, readerFps, readerStartup, lateIgnored>>

Begin(w) ==
    /\ phase[w] = "validated"
    /\ ActiveCount < ResourceLimit
    /\ phase' = [phase EXCEPT ![w] = "begun"]
    /\ UNCHANGED <<settings, runEntry, durableGeneration, readGeneration,
                  attempts, commitCount, logicalTime, snapshot, readerPhase,
                  readerTheme, readerFps, readerStartup, lateIgnored>>

ResourceExhausted(w) ==
    /\ phase[w] = "validated"
    /\ ActiveCount >= ResourceLimit
    /\ phase' = [phase EXCEPT ![w] = "failed"]
    /\ UNCHANGED <<settings, runEntry, durableGeneration, readGeneration,
                  attempts, commitCount, logicalTime, snapshot, readerPhase,
                  readerTheme, readerFps, readerStartup, lateIgnored>>

Commit(w) ==
    /\ phase[w] = "begun"
    /\ IF /\ readGeneration[w] = durableGeneration
          /\ CandidateGeneration(w) > durableGeneration
       THEN /\ settings' = Desired(w)
            /\ runEntry' = Desired(w)[3]
            /\ durableGeneration' = CandidateGeneration(w)
            /\ phase' = [phase EXCEPT ![w] = "done"]
            /\ commitCount' = [commitCount EXCEPT ![w] = @ + 1]
       ELSE /\ phase' = [phase EXCEPT ![w] = "rejected"]
            /\ UNCHANGED <<settings, runEntry, durableGeneration, commitCount>>
    /\ UNCHANGED <<readGeneration, attempts, logicalTime, snapshot, readerPhase,
                  readerTheme, readerFps, readerStartup, lateIgnored>>

Timeout(w) ==
    /\ phase[w] \notin Terminal
    /\ logicalTime = TimeBound
    /\ phase' = [phase EXCEPT ![w] = "timedout"]
    /\ UNCHANGED <<settings, runEntry, durableGeneration, readGeneration,
                  attempts, commitCount, logicalTime, snapshot, readerPhase,
                  readerTheme, readerFps, readerStartup, lateIgnored>>

Cancel(w) ==
    /\ phase[w] \notin Terminal
    /\ phase' = [phase EXCEPT ![w] = "cancelled"]
    /\ UNCHANGED <<settings, runEntry, durableGeneration, readGeneration,
                  attempts, commitCount, logicalTime, snapshot, readerPhase,
                  readerTheme, readerFps, readerStartup, lateIgnored>>

Retry(w) ==
    /\ phase[w] \in FailureTerminal
    /\ attempts[w] < MaxRetries
    /\ phase' = [phase EXCEPT ![w] = "new"]
    /\ attempts' = [attempts EXCEPT ![w] = @ + 1]
    /\ UNCHANGED <<settings, runEntry, durableGeneration, readGeneration,
                  commitCount, logicalTime, snapshot, readerPhase, readerTheme,
                  readerFps, readerStartup, lateIgnored>>

\* Completion after a deadline is recorded as ignored.  It has no authority
\* to alter durable state.
LateSuccessIgnored(w) ==
    /\ phase[w] = "timedout"
    /\ lateIgnored < Actors
    /\ lateIgnored' = lateIgnored + 1
    /\ UNCHANGED <<settings, runEntry, durableGeneration, phase, readGeneration,
                  attempts, commitCount, logicalTime, snapshot, readerPhase,
                  readerTheme, readerFps, readerStartup>>

AdvanceTime ==
    /\ logicalTime < TimeBound
    /\ logicalTime' = logicalTime + 1
    /\ UNCHANGED <<settings, runEntry, durableGeneration, phase, readGeneration,
                  attempts, commitCount, snapshot, readerPhase, readerTheme,
                  readerFps, readerStartup, lateIgnored>>

\* Readers first acquire a durable snapshot and then decode its fields.  This
\* is the specification of snapshot consistency, not a claim about the three
\* physical RegQueryValueExW calls in the current code.
BeginSnapshot ==
    /\ readerPhase = "before"
    /\ snapshot' = settings
    /\ readerPhase' = "theme"
    /\ readerTheme' = settings[1]
    /\ UNCHANGED <<settings, runEntry, durableGeneration, phase, readGeneration,
                  attempts, commitCount, logicalTime, readerFps, readerStartup,
                  lateIgnored>>

ReadSnapshotFps ==
    /\ readerPhase = "theme"
    /\ readerPhase' = "themeFps"
    /\ readerFps' = snapshot[2]
    /\ UNCHANGED <<settings, runEntry, durableGeneration, phase, readGeneration,
                  attempts, commitCount, logicalTime, snapshot, readerTheme,
                  readerStartup, lateIgnored>>

ReadSnapshotStartup ==
    /\ readerPhase = "themeFps"
    /\ readerPhase' = "done"
    /\ readerStartup' = snapshot[3]
    /\ UNCHANGED <<settings, runEntry, durableGeneration, phase, readGeneration,
                  attempts, commitCount, logicalTime, snapshot, readerTheme,
                  readerFps, lateIgnored>>

\* The message loop may be idle.  Keeping this explicit lets TLC distinguish
\* a quiescent lifecycle from a model deadlock; fairness below still governs
\* the actions needed for the liveness claim.
Idle == UNCHANGED vars

Next ==
    \/ \E w \in Workers : Read(w)
    \/ \E w \in Workers : Validate(w)
    \/ \E w \in Workers : ValidationFails(w)
    \/ \E w \in Workers : Begin(w)
    \/ \E w \in Workers : ResourceExhausted(w)
    \/ \E w \in Workers : Commit(w)
    \/ \E w \in Workers : Timeout(w)
    \/ \E w \in Workers : Cancel(w)
    \/ \E w \in Workers : Retry(w)
    \/ \E w \in Workers : LateSuccessIgnored(w)
    \/ AdvanceTime
    \/ BeginSnapshot
    \/ ReadSnapshotFps
    \/ ReadSnapshotStartup
    \/ Idle

TypeOK ==
    /\ settings \in SettingsDomain
    /\ runEntry \in BOOLEAN
    /\ durableGeneration \in 0..MaxGeneration
    /\ phase \in [Workers -> Phases]
    /\ readGeneration \in [Workers -> 0..MaxGeneration]
    /\ attempts \in [Workers -> 0..MaxRetries]
    /\ commitCount \in [Workers -> 0..1]
    /\ logicalTime \in 0..TimeBound
    /\ snapshot \in SettingsDomain
    /\ readerPhase \in ReaderPhases
    /\ readerTheme \in 0..Actors
    /\ readerFps \in 10..(10 + Actors)
    /\ readerStartup \in BOOLEAN
    /\ lateIgnored \in 0..Actors

AtomicCommit == runEntry = settings[3]
DurableGenerationMatchesPayload ==
    durableGeneration = 0
    \/ \E w \in Workers :
          /\ CandidateGeneration(w) = durableGeneration
          /\ settings = Desired(w)
          /\ runEntry = Desired(w)[3]
NoDuplicateCommit == \A w \in Workers : commitCount[w] <= 1
NoTerminalFailureCommit ==
    \A w \in Workers :
        phase[w] \in FailureTerminal => commitCount[w] = 0
SnapshotConsistent ==
    readerPhase = "done" =>
        <<readerTheme, readerFps, readerStartup>> = snapshot
NoLateCommit ==
    \A w \in Workers : phase[w] = "timedout" => commitCount[w] = 0

Terminates(w) == phase[w] \in Terminal
Termination == \A w \in Workers : (phase[w] = "new") ~> Terminates(w)

Spec ==
    /\ Init
    /\ [][Next]_vars
    /\ WF_vars(AdvanceTime)
    /\ \A w \in Workers :
          /\ WF_vars(Read(w))
          /\ WF_vars(Validate(w))
          /\ WF_vars(Begin(w))
          /\ WF_vars(Commit(w))
          /\ WF_vars(Timeout(w))
          /\ WF_vars(Retry(w))
=============================================================================
