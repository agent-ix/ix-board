---
id: US-007
title: Share a versioned focus report between clients
type: US
relationships:
  - target: "ix://agent-ix/ix-board/FR-016"
    type: traces_to
---
# US-007: Share a versioned focus report between clients

## Story

**As a** lead using an agent and a dashboard
**I want** ix-board to produce one versioned JSON focus record with canonical
selectors and status facts
**So that** both clients can present the same selected work and source age.

## Acceptance Examples (Illustrative)

### US-007-EX-1: Report a named campaign consistently

- **Given** a lead selects a named initiative
- **When** ix-board emits a focus record
- **Then** the record contains canonical Linear IDs, a schema version, report
  time, and status from one workspace snapshot

### US-007-EX-2: Leave storage to a separate service

- **Given** a client wants to store the record in a generic relay
- **When** ix-board emits the JSON record
- **Then** the client can use its published schema without the report command
  writing to the relay or choosing persistence policy

## Notes (Informative)

Relay publication, retrieval, and persistence are future work. The record
describes a selected Linear focus, not the live state of an agent session.
