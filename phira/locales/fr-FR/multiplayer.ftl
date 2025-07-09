
multiplayer = Multijoueur

connect = Se connecter
connect-must-login = Vous devez vous connecter pour accéder au mode multijoueur.
connect-success = Connecté(e) avec succès.
connect-failed = Échec de connexion.
connect-authenticate-failed = Échec d'autorisation
reconnect = Reconnexion…

create-room = Créer une salle
create-room-success = Salle créée.
create-room-failed = Impossible de créer une salle.
create-invalid-id = L'ID de la salle ne contient pas plus de 20 caractères, dont des lettres, des chiffres, - (tiret) et _ (tiret du bas).

join-room = Rejoindre la salle
join-room-invalid-id = ID de salle invalide.
join-room-failed = Impossible de rejoindre la salle.

leave-room = Quitter la salle
leave-room-failed = Impossible de quitter la salle.

disconnect = Se déconnecter

request-start = Démarrer le jeu
request-start-no-chart = Vous n'avez pas encore sélectionné de partition.
request-start-failed = Impossible de démarrer le jeu.

user-list = Utilisateurs

lock-room = { $current ->
  [true] Déverrouiller la salle
  *[other] Verrouiller la salle
}
cycle-room = { $current ->
  [true] Mode cycle
  *[other] Mode normal
}

ready = Prêt(e)
ready-failed = Impossible de se préparer.

cancel-ready = Annuler

room-id = ID de salle: { $id }

download-failed = Échec du téléchargement de la partition

lock-room-failed = Impossible de verrouiller la salle
cycle-room-failed = Échec du changement de mode de salle

chat-placeholder = Dire quelque chose...
chat-send = Envoyer
chat-empty = Le message ne peut pas être vide.
chat-sent = Envoyé
chat-send-failed = Échec de l'envoi du message.

select-chart-host-only = Seul(e) l'hôte(sse) peut sélectionner la partition.
select-chart-local = Vous ne pouvez pas sélectionner la partition locale.
select-chart-failed = Impossible de sélectionner la partition.
select-chart-not-now = Vous ne pouvez pas sélectionner de partition maintenant.

msg-create-room = `{ $user }` a créé la salle.
msg-join-room = `{ $user }` a rejoint la salle.
msg-leave-room = `{ $user }` a quitté la salle.
msg-new-host = `{ $user }` est devenu(e) le(la) nouvel(le) hôte(sse) de la salle.
msg-select-chart = L'hôte(sse) `{ $user }` a sélectionné la partition `{ $chart }` (#{ $id }).
msg-game-start = L'hôte(sse) `{ $user }` a démarré le jeu.
msg-ready = `{ $user }` est prêt(e).
msg-cancel-ready = `{ $user }` a annulé l'état prêt.
msg-cancel-game = `{ $user }` a annulé le jeu.
msg-start-playing = Jeu démarré.
msg-played = `{ $user }` a fini de jouer à: { $score } ({ $accuracy }){ $full-combo ->
  [true] , full combo.
  *[other] {""}.
}
msg-game-end = Jeu terminé.
msg-abort = `{ $user }` a abandonné le jeu
msg-room-lock = { $lock ->
  [true] Salle verrouillée.
  *[other] Salle déverrouillée.
}
msg-room-cycle = { $cycle ->
  [true] La salle est passée en mode cycle.
  *[other] La salle est passée en mode normal.
}
