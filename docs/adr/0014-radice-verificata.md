# ADR 0014 — Radice verificata: politica d'errore e supporto d'installazione

- **Stato**: accettata con il sotto-passo P0.5b; politica d'errore confermata dall'utente il 15 novembre 2026
- **Data**: 15 novembre 2026
- **Origine**: passo P0.5 del documento 0; completa l'ADR 0003

## Contesto

L'ADR 0003 ha deciso che la radice è protetta da dm-verity e che il dispositivo verificato lo crea il kernel, senza initrd. Restavano da decidere tre cose che il documento 0 non fissava: che cosa fa il sistema quando un blocco non corrisponde; se anche il supporto d'installazione è verificato; come l'installatore si accerta di ciò che ha scritto.

## Decisione

1. **Un blocco che non corrisponde ferma la macchina** (`panic_on_corruption`), e un blocco fatale del kernel la riavvia dopo dieci secondi (`panic=10`). Dal sotto-passo P0.5d il riavvio consuma un tentativo del contatore di avvio, e dopo tre tentativi il sistema torna allo slot precedente.
2. **Il supporto d'installazione è verificato anch'esso**, con etichette proprie (`dawos-inst-<versione>`, `dawos-instv-<versione>`).
3. **L'installatore rilegge e controlla** radice e albero di hash dopo averli scritti sul disco (`veritysetup verify` contro l'impronta radice), e rifiuta di concludere se non corrispondono.
4. **L'albero di hash è riproducibile**: sale e identificativo derivano dall'impronta SHA-256 dell'immagine della radice.
5. **La riga di comando del sistema installato viaggia sul supporto** in un file preparato alla costruzione; l'installatore non la ricava più modificando la propria.

## Alternative scartate

| Alternativa | Perché no |
| --- | --- |
| Errore di lettura al solo processo che ha letto il blocco (comportamento predefinito di dm-verity) | Un avvio con la radice alterata fallirebbe in modo confuso, senza un messaggio riconoscibile, e nulla riavvierebbe la macchina per far scattare il ritorno allo slot precedente. |
| `restart_on_corruption` | Riavvio immediato senza messaggio: da fuori è un riavvio inspiegabile. |
| Supporto d'installazione non verificato | Una chiavetta difettosa verrebbe scoperta, forse, dopo aver già scritto il disco. |
| Fidarsi della copia senza rileggerla | Un errore di scrittura sul disco verrebbe scoperto al primo avvio, con un blocco del kernel invece che con un errore dell'installatore. |
| Sale casuale (predefinito di `veritysetup`) | La stessa radice darebbe ogni volta un'impronta diversa: due costruzioni non sarebbero confrontabili. Il sale non è un segreto, quindi derivarlo dall'immagine non toglie nulla. |

## Conseguenze

- Un blocco alterato della radice **interrompe una sessione in corso**. È accettato: significa disco che cede o manomissione, e il codice del percorso audio è comunque già in memoria.
- Ogni blocco fatale del kernel, non solo quelli di dm-verity, porta a un riavvio dopo dieci secondi.
- Finché non esiste il ritorno allo slot precedente (sotto-passo P0.5d), una radice alterata produce un ciclo di riavvii.
- L'immagine contiene `cryptsetup` e, per la libreria di device-mapper, `lvm2`. Servirà comunque per le home cifrate.
- L'impronta radice sta sulla riga di comando: è protetta solo quando la riga di comando è firmata (sotto-passo P0.5c).

## Quando si riapre

- **Correzione degli errori (FEC).** dm-verity può affiancare all'albero dati di correzione, che riparano pochi blocchi rovinati invece di fermare la macchina. Non introdotta: va valutata quando avremo dati sull'affidabilità reale, perché oggi un singolo settore illeggibile della radice ferma il sistema.
- **Politica d'errore durante una sessione.** Se l'esperienza mostrasse che fermare la macchina è peggio che proseguire, si può distinguere tra avvio e funzionamento: il servizio di salute del sistema potrebbe segnalare l'errore e lasciare all'utente la scelta di chiudere la sessione.
- **Diagnosi dopo un blocco.** Con il punto M-05 (messaggi del kernel conservati nella memoria del firmware) i dieci secondi di `panic=10` potranno ridursi.
