# ADR 0003 — Avvio senza initrd, con prova a tempo

- **Stato**: accettata. Confermata in QEMU nel sotto-passo P0.5b (15 novembre 2026); sul 285K dall'installazione finale (P0.5f, 15 novembre 2026), con un avvio di 20 secondi di cui 14 del firmware; il ritorno all'immagine precedente è provato sul 285K nel sotto-passo P0.6e (17 novembre 2026)
- **Data**: 13 novembre 2026
- **Origine**: D-03 del documento 0

## Contesto

La radice è EROFS protetta da dm-verity, con l'impronta nella riga di comando firmata. Qualcosa deve creare il dispositivo verificato prima di montare la radice: un initrd oppure il kernel stesso, con `dm-mod.create`. La scelta pesa sull'obiettivo dei 3 secondi e su quanto codice gira prima della radice verificata.

## Decisione

Nessun initrd. Il kernel crea il dispositivo dm-verity dalla riga di comando (`dm-mod.create`, `CONFIG_DM_INIT`) e monta `/dev/dm-0`. Tutti i driver necessari per raggiungere la radice sono integrati nel kernel.

La decisione è subordinata a una **prova a tempo di un giorno** nel passo P0.5. Se la prova fallisce si ripiega su un initrd con systemd, senza ulteriori discussioni.

## Alternative scartate

- **Initrd con systemd**: la strada più battuta, con partizioni trovate automaticamente dall'impronta; costa circa 0,3–0,5 s _(stima)_ e più componenti nell'immagine. Resta il ripiego.
- **Initrd minimo scritto da noi**: rapido e didattico, ma è codice nostro in un punto critico per la sicurezza.

## Conseguenze

- La radice si indica per etichetta GPT, che contiene la versione: `PARTLABEL=dawos-root-<versione>`. Il passo P0.2 lo fa già con `root=PARTLABEL=…`, e funziona _(verificato in QEMU)_. Anche la tabella di `dm-mod.create` e il parametro `dm-mod.waitfor` accettano `PARTLABEL=` e `PARTUUID=`: durante l'avvio la ricerca dei dispositivi passa per `early_lookup_bdev` _(verificato nei sorgenti 6.18.63, `drivers/md/dm-table.c` e `block/early-lookup.c`)_. Il rischio principale della prova è quindi ridotto; resta da vederlo funzionare.
- La riga di comando non dipende dallo slot A o B, quindi la stessa UKI vale per entrambi.
- Lo sblocco di LUKS con il TPM (Fase 6) avviene nella radice vera, non prima: è possibile perché la radice non è cifrata.
- In caso di guasto all'avvio non c'è una shell di emergenza dell'initrd: la diagnosi passa dalla console e dal profilo di ripiego.

## Criteri della prova

La prova è superata se, entro un giorno di lavoro: la radice verificata si monta in QEMU e sul 285K; la manomissione di un byte impedisce l'avvio; il ritorno allo slot B funziona. Altrimenti si adotta il ripiego.

## Quando si riapre

Se la Fase 6 richiederà di sbloccare un volume prima della radice, oppure se l'avvio senza initrd rendesse instabile l'ordine di comparsa dei dispositivi.

## Aggiornamento del 15 novembre 2026

Nel sotto-passo P0.5b la prova ha dato esito positivo in QEMU, in poche ore: il kernel crea il dispositivo dm-verity dalla riga di comando con le partizioni indicate per `PARTLABEL=`, monta `/dev/dm-0`, e la manomissione di un byte della radice o dell'albero di hash ferma l'avvio. Il ripiego sull'initrd non è stato necessario.

Dei tre criteri della prova restano aperti la verifica sul 285K, rinviata all'installazione finale del passo P0.5 per scelta dell'utente, e il ritorno allo slot B (sotto-passo P0.5d). Le scelte collegate, cioè politica d'errore, supporto d'installazione verificato e controllo dopo la copia, sono nell'ADR 0014.
