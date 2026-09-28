#!/bin/sh
set -eu
export PATH=/overlay/bin:/usr/sbin:/usr/bin:/sbin:/bin
umask 077

mkdir -p /var/kdbl &&
	chmod 700 /var/kdbl
[ $? -eq 0 ] || exit 1

if ! iptables -C INPUT -i wlan0 -p tcp --dport 2222 -j ACCEPT ; then
	iptables -I INPUT 1 -i wlan0 -p tcp --dport 2222 -j ACCEPT
fi

exec kdbl -b /overlay/bin/dropbear \
	-r /overlay/etc/dropbear/dropbear_ed25519_host_key \
	-D /var/kdbl
