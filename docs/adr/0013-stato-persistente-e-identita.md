# ADR 0013 — Stato persistente e identità della macchina

- **Stato**: accettata con il sotto-passo P0.5a. Il 285K fornisce un identificativo di firmware valido (verificato il 15 novembre 2026); `var=state` è nel rapporto di avvio del 285K dall'installazione finale (P0.5f) e in ogni controllo successivo
- **Data**: 15 novembre 2026
- **Origine**: punto M-04 del documento 0

## Contesto

La radice di dawOS è un file system in sola lettura, identico su ogni macchina e, dal sotto-passo P0.5b, verificato blocco per blocco. Tutto ciò che una macchina deve ricordare va quindi altrove. Tre cose vanno decise: dove vive `/var`, come si conserva l'identità della macchina che systemd pretende in `/etc/machine-id`, e dove finisce il registro di sistema.

Un vincolo pesa sulle alternative: dal sotto-passo P0.5c la riga di comando del kernel è dentro un'immagine di avvio firmata, uguale per tutte le macchine. Nulla di specifico di una macchina può più passare da lì.

## Decisione

1. **`/var` è la partizione `state`** del disco di sistema (ext4, 32 GiB, creata da `dawos-install`). La monta un'unità scritta all'avvio da `dawos-var-generator`, che la cerca per nome sullo stesso disco della partizione radice. Sul supporto d'installazione, e dove la partizione manca, `/var` resta in memoria.
2. **L'identità della macchina deriva dal firmware**: `systemd.machine_id=firmware` sulla riga di comando, cioè l'identificativo SMBIOS della scheda madre.
3. **Il registro di systemd sta in `/var/log/journal`**, con un tetto di 1 GiB e 4 GiB sempre liberi.
4. **`/etc` resta immutabile.** I pochi file che dovranno essere modificabili (rete, nome della macchina, fuso orario) saranno collegamenti verso `/var/lib/dawos/etc/`, introdotti quando un componente ne avrà bisogno.

## Alternative scartate

| Alternativa | Perché no |
| --- | --- |
| Unità `var.mount` fissa su `/dev/disk/by-partlabel/state` | Sul supporto d'installazione monterebbe lo stato del disco da cancellare; senza la partizione l'avvio fallirebbe; con due dischi dawOS la scelta sarebbe casuale. |
| Riconoscimento automatico di systemd (`systemd-gpt-auto-generator`) | Per `/var` richiede che l'identificativo della partizione sia derivato dall'identità della macchina, e risale al disco dalla radice: con la radice su dm-verity, che ha due dispositivi sottostanti, il comportamento è _da verificare_. Troppa incertezza per un guadagno piccolo. |
| Identità scritta da `dawos-install` sulla riga di comando | Incompatibile con la riga di comando firmata del sotto-passo P0.5c. |
| Identità in un file sulla partizione `state` | systemd legge `/etc/machine-id` prima che `/var` sia montata: servirebbe un passaggio anticipato nostro, da mantenere. |
| Identità casuale a ogni avvio (comportamento senza interventi) | Il registro aprirebbe una cartella nuova a ogni avvio, e gli indirizzi di rete derivati dall'identità cambierebbero. |
| `/var` in sovrapposizione (overlay) sulla radice | È ciò che Buildroot offre in alternativa: mescola stato e immagine, e rende meno chiaro che cosa un aggiornamento sostituisce. |

## Conseguenze

- Reinstallare dawOS sulla stessa macchina conserva l'identità; sostituire la scheda madre la cambia.
- L'identità coincide con un identificativo dell'hardware. systemd raccomanda di non esporla fuori dalla macchina: i componenti di dawOS che avranno bisogno di un identificativo pubblico ne deriveranno uno con `sd_id128_get_machine_app_specific()`, come systemd prescrive. _(Da ricordare nella Fase 2, per il registro delle sessioni.)_
- Una partizione `state` illeggibile non ferma l'avvio: il rapporto di avvio riporta `var=none`.
- Gli strumenti di misura e il verbale d'installazione stanno in `/var/lib/dawos/`, quindi sulla partizione `state`.
- Una reinstallazione con `dawos-install` formatta la partizione `state`.

## Quando si riapre

- Se il 285K, o un'altra macchina di destinazione, non fornisce un identificativo SMBIOS valido: si passa all'identità conservata sulla partizione `state` con un passaggio anticipato.
- Se compare la necessità di più dischi di stato, o di cifrare la partizione `state`.
- Quando si definisce il servizio di salute del sistema, per il comportamento con la partizione `state` guasta.
