# ADR 0004 — Tre partizioni di dati

- **Stato**: accettata, con ripiego definito
- **Data**: 13 novembre 2026
- **Origine**: D-04 e C-04 del documento 0

## Contesto

Il master prompt chiede un volume XFS con reflink per `/data/shared` e `/home`, il sistema in LUKS2 sbloccato dal TPM e spazi utente cifrati con systemd-homed. Su XFS homed può cifrare solo con immagini LUKS2, perché XFS non supporta `fscrypt` _(verificato)_. Un solo volume cifrato che contiene le immagini delle home significherebbe cifrare due volte ogni dato utente.

## Decisione

Tre partizioni di dati sull'NVMe da 1 TB:

| Partizione | Dimensione | Cifratura | Contenuto |
| --- | --- | --- | --- |
| `state` | 32 GiB | LUKS2 con TPM, dalla Fase 6 | `/var`: stato, registri, record di homed. |
| `shared` | 300 GiB | LUKS2 con TPM, dalla Fase 6 | XFS con reflink: `/data/shared`. |
| `homes` | il resto, circa 650 GiB | nessuna a questo livello | XFS che ospita le immagini LUKS2 di homed; dentro ogni immagine, XFS con reflink e quote di progetto. |

Nella Fase 0 le tre partizioni esistono già ma senza cifratura: ext4 per `state`, XFS per le altre due.

## Alternative scartate

- **Un solo volume XFS dentro LUKS** con le immagini di homed dentro: doppia cifratura e doppio passaggio per dm-crypt nello streaming.
- **Rinunciare a homed** e gestire noi un volume LUKS per utente: più codice nostro in un'area delicata.

## Conseguenze

- Il reflink funziona dentro una home e dentro `shared`, non tra le due: copiare un campione dalla libreria condivisa a un progetto è una copia vera.
- Lo streaming dei progetti passa per XFS → dm-crypt → dispositivo loop → XFS → NVMe. Il costo va misurato prima di dipenderne.
- Le immagini delle home sono file sparsi con `discard`: XFS non si rimpicciolisce, ma lo spazio liberato torna al volume esterno.
- I confini delle partizioni sono fissi: sbagliare le proporzioni costa uno spostamento di dati.

## Ripiego

Se l'esperimento del passo P0.6 (prova `fio` dentro un'immagine LUKS su loop, registro delle modifiche, esperimento E-05) supera le soglie di degrado definite lì, homed userà una **partizione LUKS dedicata per utente** al posto del file immagine. Il prezzo è una dimensione fissa per utente, decisa alla creazione.

## Quando si riapre

All'esito di E-05, e nella Fase 6 quando si attiva homed con più utenti reali.
