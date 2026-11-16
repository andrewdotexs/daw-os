# ADR 0018 — Profili di avvio e regolazione all'avvio

- **Stato**: accettata con il sotto-passo P0.6a per la variante `dev`, superato il 16 novembre 2026; frequenza, stato di riposo e _fair server_ dei core live si confermano sulle misure del sotto-passo P0.6b
- **Data**: 15 novembre 2026
- **Origine**: documento 0, contraddizione C-03 e passo P0.6; ADR 0002 (isolamento statico dei core); ADR 0015, punto 5 (le varianti della riga di comando sono profili dell'immagine firmata)

## Contesto

L'isolamento dei core si chiede al kernel sulla riga di comando (`isolcpus=`, `nohz_full=`, `rcu_nocbs=`), e la riga di comando sta dentro l'immagine di avvio firmata. I numeri dei core dipendono dalla macchina; l'immagine è la stessa per tutte. Il documento 0 aveva fissato il principio (un profilo per macchina dentro l'immagine, e un profilo generico senza isolamento) ma non tre cose:

1. da dove vengono i numeri, visto che servono due volte: al kernel, all'avvio, e allo strumento che regola interrupt, frequenza e stati di riposo, dopo l'avvio;
2. come una macchina sceglie il proprio profilo, se l'immagine è uguale per tutte;
3. che cosa succede se il profilo non corrisponde alla macchina.

## Decisione

1. **Un file per profilo, una sola fonte.** `/usr/share/dawos/profiles/NOME.conf` descrive una macchina: modello del processore, numero di CPU, i quattro domini (`live`, `preemptive`, `system`, `io`), le CPU senza tick, la frequenza e lo stato di riposo dei core live, il limite di potenza. Alla costruzione `post-image.sh` ne ricava i parametri del kernel del profilo omonimo; all'avvio `dawos-tune` legge lo stesso file. Kernel e regolazione non possono essere in disaccordo.
2. **Il profilo di base è `generic`**, senza isolamento: è ciò che parte quando non è stato scelto nulla, in QEMU, sul supporto d'installazione e su una macchina sconosciuta.
3. **La scelta del profilo è un'impostazione della macchina**, scritta nella sua partizione di avvio: una riga `default dawos_*@NOME` in `loader/loader.conf`. È un modello che il boot loader confronta con le proprie voci; non nomina una versione, quindi sopravvive agli aggiornamenti.
4. **Si prova prima di fissare.** `dawos-profile --try NOME` vale per il solo avvio successivo (variabile `LoaderEntryOneShot` del boot loader, cancellata alla lettura); `dawos-profile --set NOME` vale per ogni avvio. Se un profilo impedisce l'avvio, spegnere e riaccendere riporta alla scelta precedente.
5. **La riga di comando dice quale profilo è partito**: `dawos.profile=NOME`. Il rapporto di avvio lo riporta nel campo `profile=`.
6. **`dawos-tune` verifica prima di applicare.** Confronta macchina e kernel con il profilo: modello del processore, numero di CPU, quali sono i P-core, quali CPU il kernel ha isolato e quali può lasciare senza tick. Se qualcosa non corrisponde non tocca nulla e dichiara lo stato `degraded`.
7. **Un profilo non applicato per intero è un avvio fallito.** `dawos-tune` termina con errore, l'unità fallisce, e un'unità fallita è il criterio dell'ADR 0016: un aggiornamento che rompe la regolazione torna indietro come ogni altro aggiornamento guasto.
8. **L'esito è in `/run/dawos/topology.json`** e nel campo `tune=` del rapporto di avvio: `ok`, `off` (profilo `generic`), `degraded`, `none`.
9. **Il limite di potenza si abbassa soltanto.** Se il firmware consegna un limite più alto di quello del profilo, `dawos-tune` lo riporta al valore del profilo e lo annota. Non lo alza mai. _(Scelta dell'utente, 15 novembre 2026.)_
10. **Frequenza dei core live: fissa.** Il valore di partenza è la frequenza base che il processore garantisce a qualunque carico; un valore più alto si adotta solo se una misura dimostra che regge a pieno carico entro il limite di potenza. _(Scelta dell'utente, 15 novembre 2026; la misura è nel sotto-passo P0.6b.)_
11. **La console seriale sta solo nel profilo `generic`.** _(Scelta dell'utente, 15 novembre 2026.)_
12. **Il sondaggio periodico degli errori hardware corretti è spento** sulle macchine con un profilo. Vedi «Conseguenze».
13. **Il _fair server_ è spento sui core live** (`live_fair_server=off` nel profilo). _(Scelta dell'utente, 16 novembre 2026, sulla misura del sotto-passo P0.6a; da confermare con l'esperimento E-02.)_
14. **Il 285K ha un solo profilo, `285k`, con i core live 4–7.** _(Scelta dell'utente, 16 novembre 2026: a parità di rumore è risultato più fresco dell'alternativa 2–5.)_

## Alternative scartate

| Alternativa | Perché no |
| --- | --- |
| Numeri dei core scritti due volte: nel file della riga di comando e nello strumento di regolazione | Prima o poi divergono, e il risultato è una macchina che isola certi core e ne regola altri senza dirlo. |
| Calcolare i domini all'avvio dalla sola topologia letta da `/sys` | La riga di comando è firmata e non si può calcolare a quel punto (C-03). La topologia letta resta ciò con cui il profilo viene confrontato. |
| Parametri della macchina in un'aggiunta firmata a parte (_addon_ di systemd-stub) nella partizione di avvio | L'aggiunta non viaggerebbe con gli aggiornamenti: servirebbe un quarto trasferimento, e una regolazione potrebbe restare indietro rispetto al sistema. Con i profili tutto ciò che dipende dalla versione è in un solo file firmato. |
| Profilo scelto con una voce esatta (`bootctl set-default`) | La voce contiene la versione: dopo ogni aggiornamento la scelta non corrisponderebbe più a nulla. |
| Profilo scelto dall'installatore, in automatico, dal modello del processore | Il primo avvio di un profilo su una macchina nuova è proprio quello che può andare male. Meglio che resti un gesto esplicito, provato una volta e poi fissato. |
| Applicare comunque ciò che si può quando il profilo non corrisponde | Nasconderebbe l'errore dietro un sistema che sembra regolato. |
| Un profilo non applicato che non fa fallire l'avvio | L'aggiornamento che lo causa verrebbe confermato come buono, e la macchina resterebbe senza regolazione finché qualcuno non guarda. |
| `dawos-tune` che scrive sempre il limite di potenza del profilo | Potrebbe alzarlo oltre ciò che il firmware ha deciso, per esempio dopo un abbassamento fatto apposta. |
| `mce=ignore_ce` sulla riga di comando | Spegne anche l'interrupt con cui il processore segnala gli errori corretti. Spegnere il solo sondaggio conserva la segnalazione. |

## Conseguenze

- **Passaggio alla prima immagine con i profili.** Finché sul disco c'è anche un'immagine più vecchia, senza profili, il modello `dawos_*@NOME` corrisponde solo alla nuova. Per questo `dawos-profile --set` rifiuta di fissare un profilo se l'immagine in uso è ancora in prova: altrimenti, fallita la nuova, il boot loader continuerebbe a sceglierla. Dal secondo aggiornamento in poi entrambe le immagini hanno i profili, e il ritorno automatico porta allo stesso profilo dell'immagine precedente _(ordine delle voci verificato nei sorgenti di systemd-boot 258.7: le immagini senza tentativi residui vengono elencate per ultime)_.
- **Un'immagine già confermata con un profilo che non parte non torna indietro da sola.** Il contatore dei tentativi è dell'immagine, non del profilo. È il motivo del punto 4: il profilo si prova con `--try`, che non lascia traccia.
- **La scelta del profilo non è firmata**, ma può solo indicare uno dei profili firmati: chi scrive sulla partizione di avvio può far partire `generic`, non altro.
- **Sondaggio degli errori hardware.** Ogni cinque minuti ogni CPU legge i propri registri degli errori da un timer proprio, che non si può spostare: è una visita periodica a ogni core live. Spento il sondaggio, gli errori corretti restano segnalati dall'interrupt del processore (CMCI) per i registri che lo prevedono; quelli non corretti non sono toccati. Si perde la segnalazione degli errori corretti dei registri senza CMCI _(quanti siano sul 285K è da verificare)_. L'impostazione vale per tutta la macchina.
- **In una macchina virtuale** frequenza, stati di riposo e limite di potenza non esistono: `dawos-tune` salta quei passi e lo annota. Isolamento, interrupt e code di lavoro sono applicati e verificati davvero: è ciò che `make qemu-profile-test` prova.
- **_Fair server_.** Il suo timer raggiungeva ogni core live una volta al secondo (verificato con una traccia: `dl_task_timer`); spento, il rumore misurato è zero. In cambio, un thread real-time che non si ferma mai può lasciare senza tempo i thread del kernel legati a quel core: è il rischio K-04, che l'esperimento E-02 misura. Lo spegnimento passa da `debugfs`: se un giorno il kernel verrà bloccato (D-24), va verificato che resti scrivibile all'avvio.
- **Confronto tra i due profili del 285K** (una misura da dieci minuti ciascuno, sotto carico): rumore massimo 7 µs in entrambi; temperatura del package 72 °C di media e 78 °C di punta con i core 4–7, 78 °C e 87 °C con i core 2–5.
- Quando esisterà `dawosd`, la verifica all'avvio e il file `topology.json` passano a lui (documento 0, C-03); il formato del file è il punto di contatto.

## Quando si riapre

- Con l'esperimento E-02 (sotto-passo P0.6b), per il _fair server_.
- Alla fine del sotto-passo P0.6b, per la frequenza dei core live e per lo stato di riposo (ADR 0009).
- Se sul 285K compaiono errori hardware corretti che il sondaggio avrebbe segnalato.
- Con la seconda macchina supportata: il modo di scegliere il profilo andrà rivisto se le macchine diventano molte.
