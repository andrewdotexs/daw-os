# Registri delle decisioni (ADR)

Un file per decisione. Un ADR non si riscrive: se una decisione cambia, se ne aggiunge uno nuovo che dichiara di sostituire il precedente.

| N. | Decisione | Origine | Stato |
| --- | --- | --- | --- |
| [0001](0001-due-macchine.md) | Sviluppo e destinazione su due macchine | D-01 | Accettata |
| [0002](0002-isolamento-statico-dei-core.md) | Isolamento statico dei core al boot | D-02, C-02 | Accettata |
| [0003](0003-avvio-senza-initrd.md) | Avvio senza initrd, con prova a tempo | D-03 | Accettata, da confermare in P0.5 |
| [0004](0004-partizioni-dei-dati.md) | Tre partizioni di dati | D-04, C-04 | Accettata, ripiego definito |
| [0005](0005-varianti-dev-e-prod.md) | Varianti `dev` e `prod` dell'immagine | D-05, C-05 | Accettata |
| [0006](0006-toolchain-bootlin-glibc.md) | Toolchain esterna Bootlin con glibc | D-06 | Accettata |
| [0007](0007-versioni-buildroot-e-kernel.md) | Buildroot LTS come sottomodulo e kernel fissato | D-07 | Accettata |
| [0008](0008-fcp-server-processo-separato.md) | `fcp-server` come processo separato | D-08, M-12 | Accettata |
| [0009](0009-riposo-dei-core-live.md) | Stato di riposo dei core live | D-09, K-04 | In sospeso fino a P0.6 |
| [0010](0010-licenza-gplv3-eccezione-plugin.md) | GPLv3 con eccezione per i plugin | M-11 | Accettata, parere legale da ottenere |
| [0011](0011-r04-carico-sintetico.md) | R-04 con carico sintetico di riferimento | D-16, K-02 | Accettata |

## Modello

Ogni ADR ha le stesse sezioni: _Contesto_, _Decisione_, _Alternative scartate_, _Conseguenze_, _Quando si riapre_. Le affermazioni non ovvie portano le etichette del documento 0: _(verificato)_, _(da verificare)_, _(stima)_.
