# SCION Beaconing & Path-Segment Reference

This document condenses the parts of `draft-dekater-scion-controlplane-12` that describe beacons, PCBs (Path Construction Beacons), path segment types, registration, and intra/inter-ISD beaconing. Section references (e.g., §2.3.4) point back to the draft for full detail.

## 1. Beaconing Overview

**Path Exploration (Beaconing)** is the process where an AS discovers paths to other ASes (§2.1). The Control Service of each AS is responsible for:
- Generating, receiving, and propagating **Path Construction Beacons (PCBs)** on a regular basis
- Iteratively constructing path segments through PCB accumulation

**Key Concepts:**
- **PCB**: A "traveling" path segment that accumulates AS entries as it traverses the network (§2.1, §4)
- **Path Segment**: A "snapshot" of a PCB at a given time from a particular AS's vantage point (§2.1.3)
- **Beacon Store**: Temporary storage for candidate PCBs before selection and propagation (§2.3.2)
- **Path Database**: Permanent storage for registered path segments (up, down, core) (§4)

**Two Concurrent Layers:**
1. **Intra-ISD Beaconing** (top→down): Core ASes send PCBs to children; non-core ASes extend and forward to their children until leaf ASes are reached (§2.1)
2. **Core/Inter-ISD Beaconing** (omnidirectional): Core ASes exchange PCBs across core links to discover paths between all core ASes locally and across remote ISDs (§2.1, §3.4.2)

**Peering Links:**
- PCBs do **NOT** traverse peering links directly (§2.1.1)
- Peering links are advertised as metadata in AS entries (Peer Entries) so that segment combination can include shortcuts later (§2.1.1, §2.2.2.4)
- If both ASes at either end of a peering link have registered path segments that include this peering link, it can be used during segment combination (§2.1.1)

## 2. PCB Structure and Format

### 2.1. Top-Level PCB Structure

Each PCB consists of (§2.2):
```
+-------------+------------+------------+-----+------------+
|Segment Info | AS Entry 0 | AS Entry 1 | ... | AS Entry N |
+-------------+------------+------------+-----+------------+
```

**Protobuf Format:**
```protobuf
message PathSegment {
    bytes segment_info = 1;      // Encoded SegmentInformation
    repeated ASEntry as_entries = 2;
}
```

### 2.2. Segment Information

The `segment_info` field contains basic information about the PCB (§2.2.1):

```protobuf
message SegmentInformation {
    int64 timestamp = 1;      // Creation time (seconds since POSIX epoch)
    uint32 segment_id = 2;    // 16-bit cryptographically random identifier
}
```

**Purpose:**
- `timestamp`: Set by originating core AS; expiration time of each Hop Field is computed relative to this (§2.2.1)
- `segment_id`: Required for MAC computation in Hop Fields; used for Hop Field verification in data plane (§2.2.1)

### 2.3. AS Entry Structure

Each AS entry contains complete hop information for one AS in the path segment (§2.2.2):

```
+-----------------------+------------------------------------------+
|  Unsigned Extension   |             Signed AS Entry              |
+-----------------------+------------------------------------------+
```

**Components:**
1. **Signed Component** (required): Contains header, body, and signature
2. **Unsigned Extensions** (optional): Additional metadata (§2.2.3)

**Signed AS Entry Structure:**
```
+--------------------+------------------+-----------------------------+
|     Signature      |      Header      |             Body            |
+--------------------+------------------+-----------------------------+
```

**Key Body Components:**
- **Hop Entry**: Contains Hop Field with ingress/egress interface IDs, expiration time, and MAC (§2.2.2.5)
- **Peer Entries** (optional): Advertise peering links with neighbor ISD-AS, interface IDs, and MTU (§2.2.2.4)
- **Next ISD-AS**: For intra-ISD, specifies the next AS in the path; for core, specifies the destination core AS (§2.2.2.2)

**Signing:**
- Each AS entry MUST be signed with the Control Plane AS Certificate (§2.2.2.6)
- Signature is computed over the header_and_body component (§2.2.2.6)

## 3. PCB Reception and Validation

When a Control Service receives a PCB, it performs the following checks (§2.3.1):

### 3.1. PCB Validity
- Verify the validity of the PCB (§2.2.4)
- Check that required TRC(s) and certificate(s) are available (request via API if not) (§2.3.1)
- Invalid PCBs MUST be discarded (§2.3.1)

### 3.2. Loop Avoidance (Core ASes)
- Core ASes MUST check for duplicate hop entries created by themselves or other ASes (§2.3.1)
- PCBs with loops MUST be discarded (§2.3.1)
- Core ASes SHOULD discard PCBs that were propagated by a non-core AS (§2.3.1)
- Core ASes MAY allow paths that traverse the same ISD more than once (legitimate for large geographical ISDs) (§2.3.1)

### 3.3. Incoming Interface Validation
- The last ISD-AS entry in the received PCB MUST match the ISD-AS neighbor of the interface where the PCB was received (§2.3.1)
- The corresponding link MUST be core or parent (not peering) (§2.3.1)
- If validation fails, the PCB MUST be discarded (§2.3.1)

### 3.4. Continuity Check
- When a PCB contains two or more AS entries, the receiver MUST check that every AS entry (except the last) has an ISD-AS that equals the ISD-AS of the next entry (§2.3.1)
- Beacons with continuity violations MUST be discarded (§2.3.1)

### 3.5. Storage Decision
- If validation succeeds, the Control Service decides whether to store the PCB in the Beacon Store based on selection criteria and policies (§2.3.1, §2.3.2)
- Current practice: retain all PCBs until expired or replaced by one describing the same path with a later origination time (§2.3.2)

## 4. PCB Selection Policies

An AS MUST select which PCBs to propagate further (§2.3.3). Selection can be based on:

- **AS path length**: From originator core AS to the child (non-core) AS (§2.3.3)
- **Expiration time**: Maximum value for expiration time when extending the segment (§2.3.3)
- **ISD or AS exclusion lists**: Certain ASes or ISDs that may not appear in a segment (§2.3.3)
- **ISD loops**: If permitted, allow core AS to reach other core ASes in the same ISD via third-party ISDs (§2.3.3)
- **Availability of peering links**: Number of different peering ASes from all non-core ASes on the PCB (§2.3.3)
- **Path disjointness**: AS-disjointed paths (no common upstream/core AS) or link-disjointed paths (no shared AS-to-AS link) (§2.3.3)

**Selection Policy Characteristics:**
- Can be expressed as a stateful filter of segments (§2.3.3)
- Should forward as many PCBs as possible to ensure reachability (§2.3.3)
- NOT intended as a mechanism for traffic engineering (§2.3.3)

## 5. PCB Extension Process

Every propagation interval, the Control Service (§2.1.2, §2.3.5):

1. **Selects** the best combinations of PCBs and interfaces connecting to neighboring ASes (§2.3.3)
2. **Extends** each selected PCB by appending a new AS entry
3. **Signs** the extended PCB
4. **Propagates** the extended PCB to the neighboring AS

### 5.1. AS Entry Addition

For every selected PCB and egress interface combination, the AS appends an AS entry that includes (§2.1.2, §2.3.5):

- **Hop Field**: Specifies ingress and egress interface IDs for packet forwarding through this AS in the beaconing direction (§2.1.2, §2.2.2.5)
- **Peer Entries** (optional): Information about peering links the AS wants to advertise (§2.2.2.4)
- **Next ISD-AS**: For intra-ISD, the child AS's ISD-AS; for core, the destination core AS's ISD-AS (§2.2.2.2)

### 5.2. Intra-ISD PCB Extension

The propagation process in intra-ISD beaconing includes (§2.3.5.1):

1. Select best PCBs from Beacon Store to propagate to neighboring child ASes (§2.3.5.1)
2. **MUST** add a new AS entry to every selected PCB, including:
   - Hop Entry with ingress/egress interface IDs (§2.3.5.1)
   - Any Peer Entry information the AS is configured to advertise (§2.3.5.1)
3. **MUST** sign each selected, extended PCB and append the computed signature (§2.3.5.1)
4. Propagate each extended PCB to the neighboring AS via `SegmentCreationService.Beacon` RPC (§2.3.5.1, §2.3.5.3)

### 5.3. Core PCB Extension

The propagation process in core beaconing includes (§2.3.5.2):

1. Select best PCBs to forward to neighboring core ASes (§2.3.5.2)
2. **MUST** add a new AS entry to every selected PCB which **MUST** include:
   - The egress interface to the neighboring core AS in the Hop Field component (§2.3.5.2)
   - The ISD_AS number of the neighboring core AS in the signed body component (§2.3.5.2)
3. **MUST** sign the extended PCBs and append the computed signature (§2.3.5.2)
4. Propagate the extended PCBs to neighboring core ASes via `SegmentCreationService.Beacon` RPC (§2.3.5.2, §2.3.5.3)

## 6. Intra-ISD Beaconing

### 6.1. Flow Overview

Intra-ISD beaconing creates path segments from core ASes to non-core ASes (§2.1):

1. **Initiation**: Control Services of core ASes create PCBs and send them to non-core child ASes at regular intervals (§2.1)
2. **Propagation**: Non-core child ASes receive PCBs, extend them, and forward to their child ASes (§2.1)
3. **Termination**: Process continues until PCBs reach ASes without children (leaf ASes) (§2.1)
4. **Result**: All ASes within an ISD receive path segments to reach the core ASes of their ISD (§2.1)

### 6.2. Topology Characteristics

- Typically produces an **acyclic graph** that is narrow at the top, widens towards the leaves, and is relatively shallow (§3.4.1)
- Intermediate provider ASes have many children but few parents (§3.4.1)
- Chain of intermediate providers from a leaf AS to a core AS is typically not long (e.g., local, regional, national provider, then core) (§3.4.1)

### 6.3. PCB Limits and Selection

- **Best PCB set size**: At most **50 per child link** (§2.3.4, §3.4.1)
- **Typical practice**: ~20 PCBs per child link (§2.3.4)
- **Per parent link**: Each AS receives up to **50 PCBs per active parent link** per interval (§3.4.1)
- **Trimming**: If the number of PCBs grows above 50, ASes SHOULD trim the set propagated (§3.4.1)

### 6.4. Scalability Characteristics (§3.4.1)

**Reception (Ingress):**
- AS with 100 parent links receives ≤5,000 PCBs per interval
- Assuming average length of 10 AS entries: ≈50,000 AS entries
- At 5 s intervals: ≈2.5 MB/s and 10k signature checks per second

**Propagation (Egress):**
- AS with 1,000 child links: ≤50 PCBs per link = ≤50,000 signatures per interval
- Total bandwidth: ≈25 MB/s

**Bootstrap Latency:**
- ≈(longest path length) × T/2
- For T=5 s and 10-hop path: all segments discovered in ≈25 s on average
- Fast-recovery mode can accelerate when no PCB succeeded in the last interval (§2.3.4)

**New Link Discovery:**
- New parent-child link: parent AS propagates available PCBs in next propagation event
- If child is a leaf AS: path discovery complete after at most one interval
- If child has children at distance D: learn of new link after at worst D further intervals (§3.4.1)

## 7. Core / Inter-ISD Beaconing

### 7.1. Flow Overview

Core beaconing constructs path segments between core ASes in the same or different ISDs (§2.1):

1. **Initiation**: Core AS Control Services either initiate PCBs or propagate PCBs received from neighboring core ASes (§2.1)
2. **Propagation**: PCBs are periodically sent over policy-compliant paths to discover multiple paths between any pair of core ASes (§2.1)
3. **Direction**: **Omnidirectional** - no defined direction (unlike intra-ISD's top-down) (§2.1)

### 7.2. PCB Limits and Selection

- **Best PCB set size**: At most **5 per immediate neighbor core AS** (§2.3.4)
- **Typical practice**: Each set chosen among PCBs received from each neighbor (§2.3.4)
- **Small cores**: Practice may use up to 20 (§2.3.4)

### 7.3. Scalability Characteristics (§3.4.2)

**Reception:**
- With N core ASes, each link can deliver ≤5·N PCBs per interval
- In a 1,000-node core, an AS with 300 core links may receive up to **1.5 M PCBs per interval**
- Assuming average PCB length of 6 and 60 s interval: ≈150k signature validations/s, ≈38 MB/s
- Load can be parallelized across control-service instances (§3.4.2)

**Bootstrap Latency:**
- Full connectivity obtained after number of propagation steps = network diameter
- With diameter 6 and T=60 s: full connectivity appears in ≈3 min on average (§3.4.2)

**New Link Discovery:**
- New link available to connect two ASes at distances D1 and D2 from the link after at worst (D1+D2)×T/2 (§3.4.2)

**Path Characteristics:**
- Number of distinct paths through core network is typically very large (§3.4.2)
- Shortest paths through real-world networks are relatively short (e.g., Barabási-Albert model: diameter ≈ log(N)/log(log(N))) (§3.4.2)
- Selected PCBs are likely not much longer than shortest paths (§3.4.2)

## 8. Propagation Intervals and Best PCB Set Sizes

### 8.1. Propagation Intervals

PCBs are propagated in batches at a fixed frequency known as the **propagation interval** (§2.3.4):

- **Intra-ISD beaconing**: Should be at least **5 seconds** (§2.3.4)
- **Core beaconing**: Should be at least **60 seconds** (§2.3.4)

**Fast Recovery:**
- An AS MAY attempt to forward a PCB more frequently if no PCB propagation is known to have succeeded within the last propagation interval (§2.3.4)
- Reasons: corresponding RPC failed, or no beacon was available to propagate (§2.3.4)

### 8.2. Best PCB Set Sizes

The **best PCBs set size** should be (§2.3.4):

- **Intra-ISD beaconing** (propagating to children ASes): At most **50** (§2.3.4)
  - Typical practice: ~20 (§2.3.4)
- **Core beaconing** (propagation between core ASes): At most **5 per immediate neighbor core AS** (§2.3.4)
  - Small cores: practice may use up to 20 (§2.3.4)

**Trade-offs:**
- These values reflect a tradeoff between scalability (limited by signature verification overhead) and the amount of paths discovered (§2.3.4)
- Set size should not be too low to ensure beaconing can discover a wide amount of paths (§2.3.4)

### 8.3. Beacon Store Management

- Depending on selection criteria, it may be necessary to keep more candidate PCBs than the best PCBs set size in the Beacon Store (§2.3.4)
- If this is the case, an AS should have suitable pre-selection of candidate PCBs to keep the Beacon Store capacity limited (§2.3.4)

## 9. Path Segment Types and Registration

### 9.1. Terminology

- **PCB**: A "traveling" path segment that accumulates AS entries when traversing the Internet (§2.1.3)
- **Path Segment**: A "snapshot" of a traveling PCB at a given time T from a particular AS A's vantage point (§2.1.3)
- **Registration**: The process where an AS transforms selected PCBs into path segments and adds them to relevant path databases (§4)

### 9.2. Path Segment Types

SCION defines three types of path segments (§1.4.1, §4):

1. **Up Segments**: Path segments from a non-core AS to a core AS (§4.1.2)
2. **Down Segments**: Path segments from a core AS to a non-core AS (§4.1.3)
3. **Core Segments**: Path segments between core ASes (intra-ISD or inter-ISD) (§4.2)

### 9.3. Terminating a PCB

Both up and down segments end at the AS, so transforming a PCB into a path segment "terminates" the PCB (§4.1.1).

**Termination Steps** (§4.1.1):

1. **Add final AS entry** with:
   - **Next AS MUST NOT be specified**: In Protobuf, `next_isd_as` field MUST be "0" (§4.1.1)
   - **Egress interface MUST NOT be specified**: In Protobuf, `egress` field in HopField MUST be "0" (§4.1.1)

2. **Add Peer Entries** (optional):
   - If the AS has peering links, the Control Service MAY add corresponding peer entry components (§4.1.1)
   - The egress Interface ID in the Hop Field component of each peer entry MUST NOT be specified (MUST be "0") (§4.1.1)

3. **Sign the modified PCB**: The Control Service MUST sign the modified PCB and append the computed signature (§4.1.1)

### 9.4. Up Segment Registration

Every registration period, the Control Service of a non-core AS performs (§4.1.2):

1. **Select PCBs** from candidate PCBs in the Beacon Store that it wants to transform into up segments
2. **Terminate** the selected PCBs using the steps in §4.1.1
3. **Store locally**: Add the newly created up segments to its own path database

**Purpose**: Allow infrastructure entities and endpoints in this AS to communicate with core ASes (§4.1)

### 9.5. Down Segment Registration

Every registration period, the Control Service of a non-core AS performs (§4.1.3):

1. **Select PCBs** from candidate PCBs in the Beacon Store that it wants to transform into down segments
2. **Terminate** the selected PCBs using the steps in §4.1.1
3. **Register with core**: Register the newly created down segments with the Control Services of the core ASes that originated the corresponding PCBs via `SegmentRegistrationService.SegmentsRegistration` RPC (§4.1.3, §4.3)
   - **Validation**: The first ISD-AS entry of the path segment SHOULD equal the core ISD-AS where the segment is being registered (§4.1.3)
   - **Rejection**: If not, the core AS MUST reject the segment (§4.1.3)

**Purpose**: Allow remote entities to reach this AS (§4.1)

**Note**: Up segments and down segments do not have to be equal - an AS may want different paths for communication with cores vs. being reached by others (§4.1)

### 9.6. Core Segment Registration

The core beaconing process creates path segments from core AS to core AS (§4.2):

1. **Select PCBs**: Core Control Service selects the best PCBs towards each core AS observed so far (§4.2)
2. **Terminate**: Core Control Service terminates the selected PCBs using the steps in §4.1.1 (§4.2)
3. **Store locally**: Add the newly created core segments to the Control Service path database (§4.2)

**Difference from Intra-ISD**: There is no need to register core segments with other core ASes, as each core AS will receive PCBs originated from every other core AS (§4.2)

## 10. Operational Parameters & Limits

| Parameter | Guidance | Source |
|-----------|----------|--------|
| **Intra-ISD propagation interval** | ≥5 s; fast-recovery may accelerate if last interval produced no successful beacon | §2.3.4 |
| **Core propagation interval** | ≥60 s between sends | §2.3.4 |
| **Best PCB set size (intra-ISD)** | ≤50 per child link; typical deployments use ~20 | §2.3.4, §3.4.1 |
| **Best PCB set size (core)** | ≤5 per neighbor destination (may be up to 20 in small cores) | §2.3.4 |
| **PCBs per parent link (intra-ISD)** | Up to 50 per active parent link per interval | §3.4.1 |
| **Candidate pool** | Keep more than set size only if selection policy needs it; otherwise trim Beacon Store | §2.3.4 |
| **Hop-field expiration** | Encoded duration = (1 + exp_time)·(86,400 s / 256); minimum 337.5 s. Recommended to set ≈6 h to avoid multi-hop expiry | §2.2.2.5, §3.3 |
| **PCB signing workload (intra-ISD)** | For 100 parent links: ≤5k PCBs/interval (~2.5 MB/s). For 1k child links: ≤50k signatures/interval (~25 MB/s). | §3.4.1 |
| **Core workload example** | 300 core links in 1k-node core ⇒ ≤1.5 M PCBs/interval, ≈150k sig validations/s, ≈38 MB/s | §3.4.2 |
| **Path discovery latency (intra-ISD)** | ≈(longest path length) × T/2 (e.g., 25 s for 10-hop path with T=5 s) | §3.4.1 |
| **Path discovery latency (core)** | ≈(diameter) × T/2 (e.g., 3 min for diameter 6 with T=60 s) | §3.4.2 |
| **Certificate lifetime** | 3 h–3 d recommended (≤5 d); must exceed hop expiration; coarse clock sync within minutes suffices | §3.3 |
| **Clock synchronization** | Coarse time synchronization acceptable (deviations on order of several minutes) | §3.3 |

## 11. Path Lookup Inputs

### 11.1. Segment Storage

- **Non-core ASes**: Store up segments locally in their path database (§4.1.2)
- **Core ASes**: Store down segments registered by children plus their own core segments (§4.1.3, §4.2)

### 11.2. Path Construction Process

When resolving paths, the source Control Service combines (§5.1.1):

1. **Up segments** from its local path database (if source is non-core)
2. **Core segments** fetched from reachable core ASes (possibly multi-hop through other cores)
3. **Down segments** fetched from the destination ISD's core Control Services (if destination is non-core)

This sequence ensures compliance with the inter-ISD lookup flow (§5.1.1).

## 12. Implementation Checklist for Simulator

### 12.1. PCB Propagation

- [ ] Enforce per-link PCB limits (≤50 per child link for intra-ISD, ≤5 per neighbor for core) when selecting propagation batches (§2.3.4)
- [ ] Maintain propagation timers honoring 5 s/60 s minima for intra-ISD/core respectively (§2.3.4)
- [ ] Implement fast-recovery mode: increase frequency if no PCB propagation succeeded in last interval (§2.3.4)
- [ ] Use one-hop paths for initial communication with neighboring beacon services (§2.3.5)

### 12.2. PCB Reception and Validation

- [ ] Verify PCB validity (signatures, TRCs, certificates) (§2.3.1)
- [ ] Check for loops (especially for core ASes) (§2.3.1)
- [ ] Validate incoming interface: last ISD-AS entry must match neighbor of receiving interface (§2.3.1)
- [ ] Verify link type: must be core or parent (not peering) (§2.3.1)
- [ ] Check continuity: each AS entry's ISD-AS must equal next entry's ISD-AS (§2.3.1)

### 12.3. PCB Extension

- [ ] For intra-ISD: Add AS entry with ingress/egress interface IDs and optional peer entries (§2.3.5.1)
- [ ] For core: Add AS entry with egress interface to neighboring core and neighbor ISD-AS (§2.3.5.2)
- [ ] Sign each extended PCB with AS control-plane certificate (§2.3.5.1, §2.3.5.2)
- [ ] Propagate via `SegmentCreationService.Beacon` RPC (§2.3.5.3)

### 12.4. Beacon Store Management

- [ ] Retain candidate PCBs until expired or replaced by one with later origination time (§2.3.2)
- [ ] Apply selection policies to choose best PCBs for propagation (§2.3.3)
- [ ] Trim Beacon Store if candidate pool grows beyond best PCB set size (§2.3.4)

### 12.5. Path Segment Registration

- [ ] When terminating PCBs: set `next_isd_as` to 0 and `egress` to 0 in final AS entry (§4.1.1)
- [ ] For peer entries in terminated segments: set `egress` to 0 (§4.1.1)
- [ ] Sign terminated PCBs before storing/registering (§4.1.1)
- [ ] Store up segments locally in non-core AS path database (§4.1.2)
- [ ] Register down segments with originating core AS via `SegmentRegistrationService.SegmentsRegistration` (§4.1.3)
- [ ] Validate down segments: first ISD-AS entry must match core ISD-AS where registering (§4.1.3)
- [ ] Store core segments locally in core AS path database (§4.2)

### 12.6. Path Lookup

- [ ] Request segments at AS level (ISD-AS IDs) as described in §5 (§5.1.1)
- [ ] Combine up segments (from source), core segments (from cores), and down segments (from destination ISD cores) (§5.1.1)

### 12.7. Peering Links

- [ ] Do NOT propagate PCBs over peering links (§2.1.1)
- [ ] Include peering link information in Peer Entries within AS entries (§2.1.1, §2.2.2.4)
- [ ] Use peering shortcuts during path combination if both ASes have registered segments with the peering link (§2.1.1)

### 12.8. Timing and Expiration

- [ ] Store hop-field expiration as absolute timestamps derived from segment info timestamp (§2.2.1, §2.2.2.5)
- [ ] Reject PCBs whose hop expires before they can traverse ≥5 hops (§3.3)
- [ ] Set hop expiration to ≈6 hours to avoid multi-hop expiry issues (§3.3)

---

This reference can be extended as the draft evolves; please update section pointers if the upstream document renumbers content.
