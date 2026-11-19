# ADR 0002 — Isolamento statico dei core al boot

- **Stato**: accettata; confermata il 16 novembre 2026 dall'esperimento E-03 (sotto-passo P0.6c), che ne corregge le motivazioni; confermata dalla campagna P0.7 il 19 novembre 2026: in 24 ore sotto carico nessun interrupt di dispositivo ha raggiunto un core live
- **Data**: 13 novembre 2026
- **Origine**: D-02 e C-02 del documento 0
- **Modifica una decisione vincolante**: «partizione ibrida» del master prompt, sezione Kernel

## Contesto

Il master prompt prevede `nohz_full` e `rcu_nocbs` al boot più partizioni isolate di cgroup v2 ridimensionate dai profili. Altre decisioni prevedono un cgroup per processo (limiti di memoria, regole nftables, sandbox systemd) e processi con thread in domini diversi: `dawos-engine` e ogni `dawos-plughost` hanno un thread live e thread anticipativi o di I/O.

Le due cose non convivono:

- `sched_setaffinity` è limitata alle CPU effettive del cpuset del processo, e le CPU cedute a una partizione isolata escono dal cpuset del resto dell'albero;
- un processo sta in un solo cgroup di dominio; i cgroup _threaded_ permettono di separare i thread ma applicano i controller non _threaded_ (memoria) solo alla radice del sottoalbero, togliendo i limiti per processo;
- nella 6.18 le partizioni isolate non rendono dinamici `nohz_full` e `rcu_nocbs`, che restano parametri di boot _(verificato)_.

## Decisione

Nell'MVP l'isolamento è **statico e stabilito al boot** con `isolcpus=domain,managed_irq`, `nohz_full` e `rcu_nocbs`. Ogni thread si colloca da sé con `sched_setaffinity`. I cgroup restano uno per processo e servono a memoria, numero di processi e rete, non al posizionamento sui core.

Regole collegate:

- il thread principale di ogni processo ha affinità sui domini non live; solo il thread live si sposta sui core live;
- `SCHED_RESET_ON_FORK` è sempre attivo, così un thread creato da un plugin non eredita la priorità real-time;
- `dawos-health` segnala ogni thread inatteso sui core live;
- i profili di core sono profili della UKI: cambiare profilo significa riavviare.

## Alternative scartate

- **Partizioni isolate di cgroup v2 fin da subito**: ridimensionabili, ma una partizione appartiene a un solo processo; per condividerla il motore e gli host dovrebbero stare in un unico gruppo, perdendo i limiti per processo _(motivazione corretta dopo l'esperimento E-03)_.
- **Un processo per dominio** (motore live e anticipativo separati, due host per traccia): compatibile con le partizioni, ma raddoppia i processi e complica la promozione al live.

## Conseguenze

- `isolcpus=domain` toglie il bilanciamento: sui core live ogni thread va fissato su un core preciso. È coerente con il passaggio sincrono motore ↔ host (documento 0, sezione 2.3).
- Le maschere stanno nella riga di comando firmata, quindi sono specifiche dell'hardware: profilo `285k` con le maschere, profilo `generico` senza isolamento.
- La CPU 0 non entra nel dominio live, perché su x86 il kernel toglie la CPU di avvio dall'insieme `nohz_full` per tenere il tempo di sistema _(verificato nei sorgenti 6.18.63, `kernel/time/tick-sched.c`; da osservare sul 285K nel passo P0.6)_.
- Le partizioni dinamiche diventano obiettivo di ricerca, accanto a `SCHED_DEADLINE`.

## Verifica del 16 novembre 2026 (esperimento E-03)

Le regole del kernel su cui poggia questa decisione sono state provate una per una con `dawos-cgroup-test`, sul kernel 6.18.63: in macchina virtuale e sul 285K con il profilo `285k`, con le stesse risposte (`docs/misure/fase-0/raw/cgroup-20261116T211342Z-285k`).

| Che cosa dice il «Contesto» qui sopra | Che cosa risponde il kernel |
| --- | --- |
| L'affinità è limitata alle CPU del gruppo, e le CPU di una partizione isolata escono dal resto dell'albero | **Confermato.** Da fuori non si entra e da dentro non si esce: `sched_setaffinity` risponde `Invalid argument` nei due versi. |
| Un gruppo _threaded_ può essere radice di una partizione isolata? (domanda aperta) | **Sì**, se la CPU gli viene passata dall'alto dell'albero con `cpuset.cpus.exclusive`. Sotto un gruppo che non è una partizione, no. |
| I gruppi _threaded_ tolgono i limiti per processo | **Non è esatto.** I controllori «di dominio» (memoria, I/O) non si possono accendere sotto il gruppo del processo, ma restano validi al gruppo del processo: se ogni processo ha il proprio sottoalbero, ogni processo tiene i propri limiti. |

Il motivo vero per cui le partizioni non servono a dawOS è un altro, e l'esperimento lo mostra:

- **una partizione appartiene a un solo processo**: un thread di un secondo processo non può entrare nel gruppo _threaded_ del primo (`Operation not supported`), il secondo processo non può avere una partizione sulla stessa CPU (non valida), e l'affinità non lo porta dentro;
- quindi il motore e gli host dei plugin potrebbero condividere i core live solo stando **tutti in un unico gruppo**, con un solo limite di memoria, una sola unità di systemd e nessuna regola per processo: è ciò che questa decisione vuole evitare;
- l'altra via, un core live per processo, non regge: gli host sono più dei core live, e il passaggio sincrono tra motore e host vuole che il thread dell'host giri sul core che il motore gli cede.

La disposizione scelta è stata provata nello stesso esperimento: un servizio con i suoi limiti, un thread real-time sul core live 4 collocato dal processo stesso, il thread principale su un'altra CPU.

Due conseguenze in più:

- **L'isolamento statico lascia la porta aperta.** Con il profilo `285k` la radice dei gruppi elenca i core live tra le CPU utilizzabili (`0-23`): qualunque thread può chiedere di andarci, e un gruppo con il controllore `cpuset` acceso li eredita. La regola su `dawos-health` qui sopra non è un di più: è l'unica difesa. I servizi di dawOS non devono usare `AllowedCPUs=` senza escludere i core live.
- **Il kernel era senza il controllo della memoria per gruppo** (`CONFIG_MEMCG`): i limiti di memoria per processo, che questa decisione dà per disponibili, non avrebbero avuto effetto. Entra nel kernel nello stesso sotto-passo (punto aperto A-01).

## Quando si riapre

Quando una LTS successiva offrirà `nohz_full` dinamico: senza, una partizione ridimensionabile isola dallo scheduler ma non dal tick. E a ogni cambio di versione del kernel `dawos-cgroup-test` va rieseguito: se una risposta cambia, lo dice.
