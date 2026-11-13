# ADR 0002 — Isolamento statico dei core al boot

- **Stato**: accettata
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

- **Partizioni isolate di cgroup v2 fin da subito**: ridimensionabili, ma incompatibili con i limiti per processo quando i thread di un processo stanno in domini diversi.
- **Un processo per dominio** (motore live e anticipativo separati, due host per traccia): compatibile con le partizioni, ma raddoppia i processi e complica la promozione al live.

## Conseguenze

- `isolcpus=domain` toglie il bilanciamento: sui core live ogni thread va fissato su un core preciso. È coerente con il passaggio sincrono motore ↔ host (documento 0, sezione 2.3).
- Le maschere stanno nella riga di comando firmata, quindi sono specifiche dell'hardware: profilo `285k` con le maschere, profilo `generico` senza isolamento.
- La CPU 0 non entra nel dominio live, perché su x86 viene esclusa da `nohz_full` _(da verificare sulla 6.18 nel passo P0.6)_.
- Le partizioni dinamiche diventano obiettivo di ricerca, accanto a `SCHED_DEADLINE`.

## Quando si riapre

Dopo l'esperimento del passo P0.6 (un cgroup _threaded_ può essere radice di una partizione isolata? l'affinità attraversa il confine?) e quando una LTS successiva offrirà `nohz_full` dinamico.
