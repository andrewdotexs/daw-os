# ADR 0004 — Tre partizioni di dati

- **Stato**: accettata; esperimento E-05 fatto il 17 novembre 2026: l'immagine su loop è confermata, con due vincoli; il ripiego non si adotta
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

Se l'esperimento del passo P0.6 (prova `fio` dentro un'immagine LUKS su loop, registro delle modifiche, esperimento E-05) supera le soglie di degrado definite lì, **confermate il 13 novembre 2026**, homed userà una **partizione LUKS dedicata per utente** al posto del file immagine. Il prezzo è una dimensione fissa per utente, decisa alla creazione.

**Non adottato**: vedi l'esito qui sotto.

## Esito dell'esperimento E-05

Misurato sul 285K il 17 novembre 2026 (guida: [P0.6d, parte 2](../fasi/fase-0/P0.6d-rapporto-tra-core-e-streaming-cifrato.md); misure `stream-20261117T103012Z-e05` e seguenti). Sullo stesso disco: XFS in chiaro (`s`), LUKS2 sulla partizione (`p`) e LUKS2 su loop in I/O diretto (`l`); `pn` e `ln` sono le stesse con le opzioni di dm-crypt che evitano le code di lavoro del kernel.

| Grandezza | `s` | `p` | `pn` | `l` | `ln` | Soglia |
| --- | --- | --- | --- | --- | --- | --- |
| Lettura sequenziale | 7,15 GB/s | 7,13 | 6,92 | 7,14 | 5,65 | ≥ 1,2 |
| Streaming a 400 MB/s più 6 MB/s, 99,9-esimo percentile | 0,27 ms | 0,52 | 1,06 | 0,61 | 3,29 | ≤ 20 |
| Streaming, latenza massima | 4,7 ms | 4,6 | 4,8 | 5,5 | 6,8 | ≤ 200 |
| Utilizzo del dominio di I/O | 2,2 % | 2,3 | 11,2 | 2,2 | 2,2 | ≤ 50 |
| `fdatasync`, file che cresce | 5,9 ms | 4,8 | 4,9 | 10,3–13,3 | 10,4–13,4 | ≤ 10 |
| `fdatasync`, file già scritto | 2,3 ms | — | — | 4,9 | 6,1 | ≤ 10 |
| Ritardo massimo sui core live | 4,9 µs | 57,8 | 6,6 | 57,4 | 6,4 | ≤ 20 |

- **Lo streaming regge con largo margine in ogni configurazione**: il limite è il disco, non la cifratura né il loop.
- **Senza le opzioni, dm-crypt cifra sulle code di lavoro del kernel**, che girano sui P-core e sugli E-core fuori dal dominio di I/O; il lavoro a raffiche sui P-core rallenta il risveglio dei core live (punto aperto A-33, che non riguarda solo la cifratura). Con le opzioni la cifratura resta sui core di I/O.
- **Su loop ogni `fdatasync` di un file che cresce costa il doppio**: è la sola soglia superata. Su un file già scritto costa meno della metà, anche in chiaro.

**Decisione (scelta dell'utente del 17 novembre 2026): l'immagine LUKS2 su loop è confermata, il ripiego non si adotta.** Alla lettera la regola dell'esperimento prescriveva il ripiego; lo sforamento è però del 3–30 % su una soglia stimata, riguarda una sola grandezza, e sparisce con una scelta di progetto che conviene comunque. Due vincoli entrano in questo ADR:

1. **dm-crypt si apre sempre con le opzioni `no_read_workqueue` e `no_write_workqueue`**, scritte come opzioni permanenti nell'intestazione LUKS2 di ogni immagine. Vale anche per `state` e `shared` quando saranno cifrate, nella Fase 6.
2. **Il journal del progetto riusa segmenti già scritti invece di crescere**: requisito per il motore, Fase 1.

Da verificare nella Fase 6: che `systemd-homed` applichi le opzioni permanenti quando apre l'immagine.

Altri fatti misurati che servono più avanti: il disco impiega circa 4,8 ms a svuotare la propria cache (205 `fdatasync` al secondo al più, per file); il dominio di I/O è largo, 11 % con tutta la cifratura a carico suo.

## Quando si riapre

Nella Fase 6, quando si attiva homed con più utenti reali; prima, se il progetto del journal non potesse rispettare il secondo vincolo.
