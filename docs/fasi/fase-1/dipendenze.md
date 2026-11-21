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

## Che cosa non è entrato, e perché

| Bisogno | Scelta |
| --- | --- |
| Leggere la riga di comando | Scritto a mano in `dawos-engine/src/cli.rs`: sono otto opzioni |
| Leggere `topology.json` | Scritto a mano in `dawos-rt/src/topology.rs`: serve una riga del file |

## In arrivo, da approvare al loro passo

| Passo | Bisogno | Candidato |
| --- | --- | --- |
| P1.4 | Parlare con alsa-lib (decisione D-27) | Da proporre con nome e versione dopo la verifica |
| P1.7 | Leggere il file che descrive il grafo | Da proporre; oppure un formato abbastanza semplice da leggere a mano |
