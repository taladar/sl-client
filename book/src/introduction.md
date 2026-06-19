# Introduction

This book is a high-level tour of the network protocol that
[Second Life](https://secondlife.com/) viewers and
[OpenSimulator](http://opensimulator.org/) clients use to talk to a grid. It is
written for people working on the **sl-client** Rust workspace, so alongside the
protocol explanation each chapter ends with an **"In this codebase"** note that
points at the types and modules that implement the concept being described.

You do not need to read the source to follow the protocol explanation, and you
do not need to know the protocol in detail to find your way around the crates —
the two are deliberately woven together so either entry point works.

## What we are talking to

A grid is a collection of **regions** (also called *simulators* or *sims*), each
a process responsible for a square of the virtual world. A client logs in once
against a login service, then holds a live connection to whichever region(s) its
avatar is near. The protocol has changed slowly over two decades and carries a
lot of history, which is why it mixes several transports and serialization
formats rather than one clean scheme.

Two grids speak (almost) the same protocol:

- **Second Life** (Linden Lab) is the primary target of this workspace. Some
  features — most visibly *Experiences* and the newer WebRTC voice — exist only
  here.
- **OpenSimulator** is an independent, open-source server implementation. It is
  the safe grid this workspace tests against (a local `opensim.service`), but it
  lags Second Life on some features and disables others by default.

Where behaviour differs between the two, the chapters call it out.

## Two transports

The single most important thing to understand up front is that the protocol uses
**two transports at once**, for two different kinds of traffic:

| Transport | Used for | Character |
|-----------|----------|-----------|
| **LLUDP** (UDP) | Real-time, high-volume scene and agent traffic: object updates, avatar movement, terrain, chat, sound | Custom framing with optional per-packet reliability; loss-tolerant |
| **HTTP + CAPS** (TCP/TLS) | Bulk and must-not-be-lost data: inventory, login, materials, voice provisioning, the asynchronous event queue | Request/response (and one long-poll); ordinary HTTPS |

"LLUDP" is the Linden Lab UDP protocol: a thin reliability and framing layer
over UDP carrying *messages* defined by a shared *message template*. "CAPS" is
short for *capabilities* — per-session, unguessable HTTPS URLs the region hands
out, each granting access to one server feature.

Modern features lean on CAPS; the oldest features are still LLUDP messages; many
features use both (for example, inventory has a legacy UDP path and a modern
HTTP path). A working client must speak both transports and know which to use
for what.

## How this book is organised

The book is split into the same two layers the protocol itself separates:

- **[Communication Layer](comms/index.md)** — the plumbing that every feature is
  built on: sessions, the LLUDP transport and its reliability, circuits, CAPS
  and the event queue, the LLSD data format, and the message template that
  defines every UDP message.
- **[Content Layer](content/index.md)** — the actual features carried over that
  plumbing: login, teleport, inventory, chat, the 3D world (objects, terrain,
  parcels, avatars), sound/music/media, and the rest (groups, economy, profiles,
  appearance, friends, experiences, materials).

Read the [Architecture](architecture.md) chapter next for a map of how the
sl-client crates layer up, then dip into whichever topic you need. A
[Glossary](appendix/glossary.md) collects the recurring identifiers and jargon,
and [References](appendix/references.md) points at the upstream sources of
truth.
