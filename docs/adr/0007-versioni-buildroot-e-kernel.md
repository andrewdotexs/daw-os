# ADR 0007 — Buildroot LTS come sottomodulo e kernel fissato

- **Stato**: accettata
- **Data**: 13 novembre 2026
- **Origine**: D-07 del documento 0

## Contesto

Una campagna di misura vale solo se si sa esattamente su cosa è stata fatta. Buildroot e il kernel escono con versioni correttive frequenti; aggiornarli in mezzo a una fase rende i risultati non confrontabili.

## Decisione

- **Buildroot 2026.02.x LTS** come sottomodulo git nella cartella `buildroot/`, fissato al tag `2026.02.3` (commit `679b9ead7620`, 16 giugno 2026), l'ultimo della serie alla data di questo ADR _(verificato)_.
- **Kernel 6.18.63**, l'ultima 6.18.y alla data di questo ADR _(verificato su kernel.org)_, con impronta SHA-256 nel file `external/patches/linux/linux.hash` e controllo delle impronte obbligatorio (`BR2_DOWNLOAD_FORCE_CHECK_HASHES`).
- Sottomodulo e kernel **si aggiornano solo alla chiusura di una fase**, con un commit dedicato, mai durante una campagna di misura.
- La migrazione alla LTS successiva del kernel si valuta quando sarà designata e avrà almeno tre mesi di versioni correttive.

## Alternative scartate

- **Buildroot scaricato al volo dallo script di costruzione**: nessun sottomodulo, ma la versione non è registrata nella storia del repository.
- **Ultima versione stabile di Buildroot** (2026.08): tre mesi di supporto soltanto.
- **Kernel predefinito di Buildroot**: nel tag 2026.02.3 è il 6.19.14 _(verificato)_, non una LTS.

## Conseguenze

- Dopo il clone serve `git submodule update --init`; `make check-env` controlla che il sottomodulo sia sul tag atteso.
- Le versioni dei pacchetti derivano dal tag: systemd 258.7 _(verificato)_.
- L'impronta del kernel protegge dallo scaricamento di un archivio alterato; cambia insieme al numero di versione.
- Le correzioni di sicurezza del kernel arrivano con ritardo, fino alla chiusura della fase in corso. Per una macchina di sviluppo senza servizi esposti è accettabile; per `prod` la regola andrà rivista.

## Quando si riapre

Alla chiusura di ogni fase (aggiornamento ordinario) e nella Fase 6, per definire la politica di aggiornamento di `prod`.
