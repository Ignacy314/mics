#!/bin/sh

sudo -i -u test bash << EOF
mkdir -p \$HOME/andros
cd \$HOME/andros
mkdir -p data
mkdir -p data/data
mkdir -p data/i2s
mkdir -p data/umc
mkdir -p log
mkdir -p samba
rm -rf andros
git clone https://github.com/Ignacy314/mics andros

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
cd andros
ln -s \$HOME/andros/andros/run.sh /usr/local/bin/andros
echo -e "#!/bin/bash
echo \$(ip a s wlan0 | grep ether | egrep -o ..:..:..:..:..:.. | head -1) > \$HOME/andros/mac
echo \$(ip -4 -o a | grep wlan | egrep -o '192\.168\.[0-9]{1,3}\.[0-9]{1,3}' | head -1) > \$HOME/andros/ip" > \$HOME/save_mac_ip.sh
(crontab -l 2>/dev/null; echo "@reboot \$HOME/save_mac_ip.sh") | crontab -
EOF

chmod +x /home/test/save_mac_ip.sh
cp /home/test/save_mac_ip.sh /etc/init.d/
cp /home/test/andros/andros/ANDROSi2s.dtbo /boot/firmware/overlays
cp -f /home/test/andros/andros/config.txt /boot/firmware/config.txt
chmod +x run.sh

apt-get install -y samba samba-common-bin
echo -e "[global]
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

apt-get install -y libasound2-dev
apt-get install -y libwebkit2gtk-4.0
apt-get install -y libssl-dev
# sudo apt-get install cmake;
# sudo apt-get install gfortran;

sudo -i -u test bash << EOF
cargo install --path \$HOME/andros/andros --locked
echo -e "#!/bin/sh
cd \$HOME/andros/andros
git pull
cargo install --path \$HOME/andros/andros --locked" > \$HOME/update.sh
echo -e "#!/bin/sh
sleep 10
while true; do andros; sleep 5; done" > \$HOME/run_andros.sh
(crontab -l 2>/dev/null; echo "@reboot \$HOME/update.sh") | crontab -
(crontab -l 2>/dev/null; echo "@reboot \$HOME/run_andros.sh") | crontab -
EOF

nmcli connection add type gsm ifname '*' apn internet user internet password internet connection.autoconnect yes

chmod +x /home/test/update.sh
chmod +x /home/test/run_andros.sh
