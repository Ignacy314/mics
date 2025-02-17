#!/bin/sh

# mkdir -p data/data
# mkdir -p data/i2s
# mkdir -p data/umc
# mkdir -p samba
# echo -e '#!/bin/bash
# sleep 10
# echo \$(ip a s wlan0 | grep ether | egrep -o ..:..:..:..:..:.. | head -1) > \$HOME/andros/mac
# echo \$(ip -4 -o a | grep wlan | egrep -o "192\.168\.[0-9]{1,3}\.[0-9]{1,3}" | head -1) > \$HOME/andros/ip' > $HOME/save_mac_ip.sh

apt-get update -y
apt-get upgrade -y

sudo -i -u test bash << EOF
mkdir -p \$HOME/andros
cd \$HOME/andros
mkdir -p data
mkdir -p log
rm -rf andros
git clone https://github.com/Ignacy314/mics andros

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

echo '#!/bin/bash
while ! [ "\$(ping -c 1 google.com)" ]; do
  sleep 1
done
echo \$(ip a s wlan0 | grep ether | egrep -o ..:..:..:..:..:.. | head -1) > \$HOME/andros/mac
echo \$(ip -4 -o a | grep wg0 | egrep -o "10\.66\.66\.[0-9]{1,3}" | head -1) > \$HOME/andros/ip' > \$HOME/save_mac_ip.sh
chmod 744 /home/test/save_mac_ip.sh
EOF
# (crontab -l 2>/dev/null; echo "@reboot $HOME/save_mac_ip.sh") | crontab -

nmcli connection add type gsm ifname '*' apn internet user internet password internet connection.autoconnect yes
cp /home/test/andros/andros/ANDROSi2s.dtbo /boot/firmware/overlays
cp -f /home/test/andros/andros/config.txt /boot/firmware/config.txt

apt-get install -y samba samba-common-bin
echo "[global]
server string = Andros Data
workgroup = ANDROS
log file = /home/test/samba/log.%m
max log size = 50

[andros]
path = /home/test/andros
browseable = yes
writeable = yes
read only = no
valid users = test
public = no" > /etc/samba/smb.conf
(echo "password"; sleep 1; echo "password") | smbpasswd -s -a test

systemctl restart smbd
systemctl enable --now ssh
systemctl enable --now wayvnc

bash -c "echo 'pps-gpio' >> /etc/modules"
apt-get install -y libasound2-dev
apt-get install -y libwebkit2gtk-4.0
apt-get install -y libssl-dev
apt-get install -y pps-tools gpsd gpsd-clients
apt-get install -y chrony

echo 'START_DAEMON="true"
USBAUTO="true"
DEVICES="/dev/ttyAMA0 /dev/pps0"
GPSD_OPTIONS="-n"' > /etc/default/gpsd

echo 'refclock PPS /dev/pps0 lock NMEA refid PPS precision 1e-7' >> /etc/chrony/chrony.conf

systemctl enable --now chrony
# apt-get install cmake;
# apt-get install gfortran;

# echo -e '#!/bin/bash
# sleep 2
# cd \$HOME/andros/andros
# git pull
# cargo install --path \$HOME/andros/andros --locked
# sleep 10
# echo \$(ip a s wlan0 | grep ether | egrep -o ..:..:..:..:..:.. | head -1) > \$HOME/andros/mac
# echo \$(ip -4 -o a | grep wlan | egrep -o "192\.168\.[0-9]{1,3}\.[0-9]{1,3}" | head -1) > \$HOME/andros/ip
# echo "start andros" > \$HOME/andros_started
# while true; do /home/test/.cargo/bin/andros; sleep 5; done' > $HOME/update.sh
sudo -i -u test bash << EOF
sh \$HOME/save_mac_ip.sh
cargo install just
cargo install --path \$HOME/andros/andros --locked
(crontab -l 2>/dev/null; echo "@reboot \$HOME/andros/andros/update.sh") | crontab -
EOF
