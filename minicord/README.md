# minicord

Chat estilo Discord, ultra liviano. Un solo .exe (servidor + cliente web incluido).

## Obtener el .exe
Opcion A (Windows): instalar Rust desde https://rustup.rs y hacer doble click en build.bat. El exe queda en dist\minicord.exe.
Opcion B (sin instalar nada): subir esta carpeta a un repo de GitHub; en la pestaña Actions, el workflow "build-exe" genera minicord.exe para descargar.

## Usar
Ejecutar minicord.exe: abre el navegador en http://localhost:3000. Para jugar en red, los demas entran a http://IP-DE-TU-PC:3000.
Puerto distinto: variable de entorno ADDR, por ejemplo ADDR=0.0.0.0:8080.

## Que incluye
Servidores con codigo de invitacion, canales, mensajes en tiempo real, historial, usuarios en linea.
Todavia no: contrasenas, mensajes directos, roles, voz.
