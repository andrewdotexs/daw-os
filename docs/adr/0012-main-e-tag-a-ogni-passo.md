# ADR 0012 — Unione in `main` e tag alla chiusura di ogni passo

- **Stato**: accettata
- **Data**: 13 novembre 2026
- **Origine**: proposta dell'utente durante il passo P0.4b

## Contesto

Fino al passo P0.4a tutto il lavoro della Fase 0 si è accumulato su un solo ramo, con `main` fermo al commit iniziale. Il ramo contiene anche commit intermedi dichiaratamente non provati («bozza», «in prova»). Se un passo successivo rompe qualcosa, non esiste un punto a cui tornare con certezza.

## Decisione

1. **`main` contiene solo passi chiusi.** Un passo è chiuso quando il suo criterio di accettazione è stato verificato e registrato nella guida del passo.
2. **Alla chiusura di ogni passo** lo stato accettato viene unito in `main` con un commit di unione esplicito (senza avanzamento rapido), e marcato con un **tag annotato**.
3. **Nomi dei tag**: `p<fase>.<passo>`, con una lettera se il passo è stato diviso: `p0.1`, `p0.2`, `p0.3`, `p0.4a`, `p0.4b`.
4. **Il commit che si unisce è quello accettato**, non la punta del ramo: se sul ramo c'è già la bozza del passo seguente, resta fuori da `main` finché non viene accettata a sua volta.
5. **Il ramo di lavoro continua** dopo ogni unione; non viene ricreato.

Lo strumento è `tools/close-step.sh`, che fa unione, tag e invio in un comando, e controlla che ciò che arriva in `main` sia esattamente lo stato accettato.

## Chi fa cosa

La sessione in cui lavora l'assistente può scrivere solo sul proprio ramo di lavoro: l'invio su `main` e dei tag le è stato rifiutato _(verificato il 13 novembre 2026)_. Quindi:

- l'assistente registra la chiusura sul ramo di lavoro e indica il comando, con il commit esatto e il testo dell'accettazione;
- **l'utente esegue il comando** dal proprio clone.

## Prima volta

Il commit di unione e il tag portano il nome di chi li crea, quindi git deve conoscere la tua identità. Si imposta una volta sola:

```bash
git config --global user.name "Nome Cognome"
```

```bash
git config --global user.email "indirizzo@esempio.it"
```

L'indirizzo compare nella storia pubblica del repository. Se non vuoi mostrare quello personale, GitHub ne offre uno apposito, nella forma `<numero>+<utente>@users.noreply.github.com`, indicato nelle impostazioni dell'account alla voce _Emails_.

Serve anche che il clone possa **scrivere** su GitHub. GitHub non accetta la password dell'account per le operazioni di git: il modo più semplice è usare la chiave SSH che esiste già per il 285K.

Mostrare la chiave pubblica e copiarla:

```bash
cat ~/.ssh/id_ed25519.pub
```

Aggiungerla su GitHub, in _Settings_ → _SSH and GPG keys_ → _New SSH key_. Poi verificare:

```bash
ssh -T git@github.com
```

La risposta attesa comincia con `Hi <utente>! You've successfully authenticated`. Infine far usare SSH al clone:

```bash
git remote set-url origin git@github.com:andrewdotexs/daw-os.git
```

Da quel momento anche `git pull` passa per SSH. Il sottomodulo di Buildroot non cambia: resta su HTTPS, in sola lettura.

## Come si usa

Chiudere un passo:

```bash
tools/close-step.sh p0.3 bd95317 "P0.3: Linux 6.18 PREEMPT_RT kernel and measurement tools" "Accepted on 2026-11-13: make qemu-test reports realtime=1."
```

Vedere i passi chiusi:

```bash
git log --first-parent --oneline main
```

Tornare a un passo noto, per esempio per ricostruire l'immagine di allora:

```bash
git checkout p0.3 && git submodule update --init
```

Tornare al lavoro in corso:

```bash
git checkout claude/affectionate-faraday-5le4g4
```

## Alternative scartate

- **Avanzamento rapido di `main`** fino al commit accettato: più semplice, ma `main` erediterebbe tutti i commit intermedi e non si distinguerebbero i passi.
- **Una richiesta di unione (pull request) per passo**: aggiunge una revisione che oggi nessuno farebbe; si può adottare quando ci saranno altri contributori.
- **Solo tag, senza toccare `main`**: i punti di ritorno ci sarebbero, ma `main` resterebbe vuoto e chi clona il repository non troverebbe nulla.

## Conseguenze

- `git log --first-parent main` diventa l'elenco dei passi chiusi, uno per riga.
- Lo script lavora in una copia di lavoro temporanea: i file della cartella corrente non vengono toccati, quindi non provoca ricostruzioni dell'immagine.
- Tornando a un tag vecchio, la cartella `output/` può contenere pacchetti costruiti con opzioni diverse: per una ricostruzione fedele serve `make clean`.
- I primi quattro passi vengono chiusi a posteriori, scegliendo per ciascuno l'ultimo commit che non contiene ancora la bozza del passo seguente:

| Tag | Commit | Stato accettato |
| --- | --- | --- |
| `p0.1` | `d11e1ac` | Ambiente WSL2 verificato; è anche il commit da cui è stata costruita la prima immagine sul PC di sviluppo. |
| `p0.2` | `2548337` | Stesso codice di `p0.1`, più la documentazione dell'esito. |
| `p0.3` | `bd95317` | Kernel real-time, come provato con KVM. |
| `p0.4a` | `a04f94e` | Sistema come installato sul 285K. |

## Quando si riapre

Quando ci saranno altri contributori, o se la sessione dell'assistente otterrà il permesso di scrivere su `main`.
