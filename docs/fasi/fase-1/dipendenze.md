# Fase 1 — Dipendenze esterne del motore

Regola (domanda D-18, decisa il 21 novembre 2026): ogni crate esterno si approva uno per uno, con la versione e il motivo, e si scrive qui. La versione è fissata in `crates/Cargo.toml` con il segno di uguale, e `crates/Cargo.lock` ne tiene la somma di controllo.

I sorgenti non stanno nel repository: `make engine-vendor` li scarica una volta in `crates/vendor`, verificandoli contro `Cargo.lock`. Da lì in poi né la costruzione sul PC né quella dell'immagine toccano la rete.

## Il compilatore

| Che cosa | Versione | Perché questa |
| --- | --- | --- |
| Rust | 1.88.0 | È la versione che Buildroot 2026.02.3 installa per costruire l'immagine (pacchetto `rust-bin`) _(verificato in `buildroot/package/rust-bin/rust-bin.mk`)_. `crates/rust-toolchain.toml` fissa la stessa sul PC di sviluppo: ciò che compila da una parte compila dall'altra |

## I crate

| Crate | Versione | Licenza | Dove si usa | Perché |
| --- | --- | --- | --- | --- |
| `libc` | 0.2.191 | MIT oppure Apache-2.0 | Solo in `dawos-rt`, modulo `sys` | Le chiamate alla libreria C e al kernel (orologio, attesa, core e priorità del thread, blocco della memoria) con le strutture dati giuste per l'architettura. Scriverle a mano vorrebbe dire ricopiare quelle strutture, con il rischio di sbagliarle. Nessuna dipendenza a sua volta; chiede Rust 1.65 o successivo _(verificato nell'indice di crates.io il 21 novembre 2026)_ |

| `alsa` | 0.12.1 | Apache-2.0 oppure MIT | Solo in `dawos-audio-io` | Involucro sottile e sicuro di alsa-lib (decisione D-27; scelta dell'utente del 21 novembre 2026): `mmap`, flussi collegati, attesa, stato, senza `unsafe` nostro. Ha anche un modulo che parla con il kernel senza alsa-lib: ne uso la lettura della pagina di stato, e servirà per il confronto del passo P1.8 |
| `alsa-sys` | 0.6.1 | MIT | Dipendenza di `alsa` | Le dichiarazioni di alsa-lib, già generate: non serve `bindgen` |
| `bitflags` | 2.13.2 | MIT oppure Apache-2.0 | Dipendenza di `alsa` | Insiemi di opzioni |
| `cfg-if` | 1.0.5 | MIT oppure Apache-2.0 | Dipendenza di `alsa` | Compilazione condizionata |
| `pkg-config` | 0.3.34 | MIT oppure Apache-2.0 | Solo per costruire `alsa-sys` | Chiede a `pkg-config` dov'è alsa-lib |
| `cc`, `shlex`, `find-msvc-tools` | 1.11.3, 2.0.1, 0.1.15 | MIT oppure Apache-2.0 | Solo per costruire `alsa-sys` | Compilano una prova che sulle macchine a 64 bit non viene eseguita _(verificato nello script di costruzione)_ |

`alsa` chiede sul PC di sviluppo gli header di alsa-lib (`libasound2-dev`) e `pkg-config`; nell'immagine usa alsa-lib 1.2.15.3 di Buildroot. I sorgenti in `crates/vendor` passano da 6,6 a 9,2 MiB.

## Che cosa non è entrato, e perché

| Bisogno | Scelta |
| --- | --- |
| Leggere la riga di comando | Scritto a mano in `dawos-engine/src/cli.rs`: sono otto opzioni |
| Leggere `topology.json` | Scritto a mano in `dawos-rt/src/topology.rs`: serve una riga del file |

## Programmi di altri nell'immagine, non nel motore

Non sono crate e il motore non li collega: stanno nell'immagine accanto a lui. Decisione e ragioni nell'[ADR 0024](../../adr/0024-interfaccia-fcp-server-e-firmware.md) (passo P1.12, 24 novembre 2026).

| Pacchetto | Versione | Licenza | Perché |
| --- | --- | --- | --- |
| `fcp-support` (`fcp-server`, `fcp-tool`) | commit `02be3c9` del 5 novembre 2026, quattro dopo l'etichetta `1.0beta1` | GPL-3.0-or-later | Senza `fcp-server` router, mixer e preamplificatori della Scarlett di quarta generazione non si leggono né si scrivono |
| `scarlett4-firmware` | commit `183d5ec`, etichetta `1.1`: firmware 3.0.2778.0, il solo file della 18i20 | Di Focusrite: ridistribuzione senza modifiche, solo per prodotti Focusrite. **Non è software libero** | Un firmware noto sotto ogni misura; si scrive nell'interfaccia solo a mano |

## In arrivo, da approvare al loro passo

| Passo | Bisogno | Candidato |
| --- | --- | --- |
| P1.7 | Leggere il file che descrive il grafo | Da proporre; oppure un formato abbastanza semplice da leggere a mano |
