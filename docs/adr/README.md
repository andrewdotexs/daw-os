# Registri delle decisioni (ADR)

Un file per decisione. Un ADR non si riscrive: se una decisione cambia, se ne aggiunge uno nuovo che dichiara di sostituire il precedente.

| N. | Decisione | Origine | Stato |
| --- | --- | --- | --- |
| [0001](0001-due-macchine.md) | Sviluppo e destinazione su due macchine | D-01 | Accettata |
| [0002](0002-isolamento-statico-dei-core.md) | Isolamento statico dei core al boot | D-02, C-02 | Accettata; confermata da E-03 il 16 novembre 2026 e dalla campagna P0.7 |
| [0003](0003-avvio-senza-initrd.md) | Avvio senza initrd, con prova a tempo | D-03 | Accettata; confermata in QEMU (P0.5b) e sul 285K (P0.5f, P0.6e) |
| [0004](0004-partizioni-dei-dati.md) | Tre partizioni di dati | D-04, C-04 | Accettata; E-05 fatto: loop confermato con due vincoli |
| [0005](0005-varianti-dev-e-prod.md) | Varianti `dev` e `prod` dell'immagine | D-05, C-05 | Accettata |
| [0006](0006-toolchain-bootlin-glibc.md) | Toolchain esterna Bootlin con glibc | D-06 | Accettata |
| [0007](0007-versioni-buildroot-e-kernel.md) | Buildroot LTS come sottomodulo e kernel fissato | D-07 | Accettata; kernel 6.18.64; regola per le patch fuori albero |
| [0008](0008-fcp-server-processo-separato.md) | `fcp-server` come processo separato | D-08, M-12 | Accettata |
| [0009](0009-riposo-dei-core-live.md) | Stato di riposo dei core live | D-09, K-04 | Accettata il 16 novembre 2026: C1, senza attesa attiva; confermata dalla campagna P0.7 |
| [0010](0010-licenza-gplv3-eccezione-plugin.md) | GPLv3 con eccezione per i plugin | M-11 | Accettata, parere legale da ottenere |
| [0011](0011-r04-carico-sintetico.md) | R-04 con carico sintetico di riferimento | D-16, K-02 | Accettata |
| [0012](0012-main-e-tag-a-ogni-passo.md) | Unione in `main` e tag alla chiusura di ogni passo | Proposta dell'utente | Accettata |
| [0013](0013-stato-persistente-e-identita.md) | Stato persistente e identità della macchina | M-04 | Accettata; identificativo del firmware e `var=state` verificati sul 285K |
| [0014](0014-radice-verificata.md) | Radice verificata: politica d'errore e supporto d'installazione | P0.5 | Accettata; politica d'errore confermata dall'utente |
| [0015](0015-secure-boot.md) | Secure Boot: che cosa si firma, con quale chiave, con quali strumenti | P0.5 | Accettata per `dev`; D-23 e D-24 rinviate d'accordo con l'utente |
| [0016](0016-valutazione-dell-avvio.md) | Valutazione dell'avvio e ritorno all'immagine precedente | P0.5 | Accettata |
| [0017](0017-aggiornamenti.md) | Aggiornamenti: meccanismo, firma, trasporto, versioni | Modifica 5 dell'utente | Accettata per `dev` |
| [0018](0018-profili-di-avvio-e-regolazione.md) | Profili di avvio e regolazione all'avvio | C-03, P0.6 | Accettata per `dev`; il profilo `285k` è la configurazione certificata dalla campagna P0.7 |
| [0019](0019-apertura-del-sistema-utilita-native-e-vm-linux.md) | Apertura del sistema: utilità native e VM Linux | Documento di progetto, ADR-033 | Accettata il 18 novembre 2026; nell'immagine solo la predisposizione del kernel, dentro il kernel da P1.0 |
| [0020](0020-plugin-linux-di-terze-parti.md) | Plugin Linux di terze parti | Documento di progetto, ADR-034 | Accettata il 18 novembre 2026; nulla ancora nell'immagine |
| [0021](0021-core-live-senza-kernel.md) | Core live senza kernel | Documento di progetto, ADR-038 | Accettata il 19 novembre 2026, con tre integrazioni; precisata il 21 novembre (risveglio dalla scheda, regola sulle syscall per fase) |
| [0022](0022-driver-grafico-all-avvio.md) | Driver grafico caricato a ogni avvio | P1.0, A-43 | Accettata il 20 novembre 2026 per il profilo `285k`; cambia la decisione di A-02; misura lunga da fare |
| [0023](0023-protezione-dal-thread-che-non-cede.md) | Protezione dal thread live che non cede | K-04, A-18, prompt della Fase 1 | Accettata il 21 novembre 2026; verificata sul 285K con la prova d'urto del passo P1.3 |
| [0024](0024-interfaccia-fcp-server-e-firmware.md) | L'interfaccia di quarta generazione: `fcp-support` e firmware nell'immagine | P1.12, D-28 | Accettata il 24 novembre 2026; da confermare con le misure sul 285K |
| [0025](0025-grafo-assegnazione-statica.md) | Il grafo: ogni nodo ha il suo core, deciso alla compilazione | D-10, P1.7 | Accettata il 25 novembre 2026; il compilatore è in P1.7a, la misura in P1.7d |
| [0026](0026-driver-audio-di-serie-con-iso-asap.md) | Driver audio di serie, con l'uscita consegnata «appena possibile»; `dawos-uac` non si scrive in Fase 1 | Q-14, A-50, A-69, P1.8 | Accettata il 26 novembre 2026, in prova fino alla campagna di 24 ore (P1.10); dal 27 novembre la patch vale solo per i flussi a retroazione implicita |

## Corrispondenza con il documento di progetto

Il documento di progetto numera i propri ADR da ADR-001 in su (a oggi fino ad ADR-038), con una numerazione diversa da questa. Una decisione del documento di progetto entra nel repository con il **primo numero libero di qui** e porta nell'intestazione il rimando al numero di là: così non esistono due «0033» con significati diversi. Nel testo, «ADR 0019» (quattro cifre, senza trattino) è sempre un ADR del repository; «ADR-033» (tre cifre, con il trattino) è sempre un ADR del documento di progetto.

Questa tabella si aggiorna ogni volta che ne entra uno.

| Repository | Documento di progetto | Titolo | Entrato il |
| --- | --- | --- | --- |
| [0019](0019-apertura-del-sistema-utilita-native-e-vm-linux.md) | ADR-033 | Apertura del sistema: utilità native e VM Linux | 18 novembre 2026 |
| [0020](0020-plugin-linux-di-terze-parti.md) | ADR-034 | Plugin Linux di terze parti | 18 novembre 2026 |
| [0021](0021-core-live-senza-kernel.md) | ADR-038 | Core live senza kernel | 19 novembre 2026 |

Anche le questioni aperte del documento di progetto (Q-xx) entrano con il primo numero libero del [registro dei punti aperti](../02-punti-aperti.md):

| Repository | Documento di progetto | Che cosa | Entrato il |
| --- | --- | --- | --- |
| A-48 | Q-12 | Verifica del firmware all'avvio | 19 novembre 2026 |
| A-49 | Q-13 | Patch del kernel: prova del rinvio degli interrupt tra core | 19 novembre 2026 |
| A-50 | Q-14 | Driver audio: un driver proprio solo dopo la misura | 19 novembre 2026 |

Gli ADR da 0001 a 0018 sono nati nel repository, dalla revisione del documento 0 e dai passi della Fase 0. Se qualcuno di loro ha un corrispondente nel documento di progetto, qui non è registrato: quel documento non è nel repository.

## Modello

Ogni ADR ha le stesse sezioni: _Contesto_, _Decisione_, _Alternative scartate_, _Conseguenze_, _Quando si riapre_. Le affermazioni non ovvie portano le etichette del documento 0: _(verificato)_, _(da verificare)_, _(stima)_.
