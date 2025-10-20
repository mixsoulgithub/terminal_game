#! /bin/bash
mkdir -p ~/terminal_game_envs/micromamba
cd ~/terminal_game_envs/micromamba
# 最简单的方法：临时修改sources.list使用HTTP镜像源
echo "deb http://mirrors.aliyun.com/ubuntu/ focal main restricted universe multiverse" > /etc/apt/sources.list
echo "deb http://mirrors.aliyun.com/ubuntu/ focal-security main restricted universe multiverse" >> /etc/apt/sources.list
echo "deb http://mirrors.aliyun.com/ubuntu/ focal-updates main restricted universe multiverse" >> /etc/apt/sources.list

apt update
apt install -y wget

#检测是否安装了micromamba
if [micromamba -v]
then
    echo "micromamba is installed"
else
    echo "micromamba is not installed, install micromamba now"
    wget https://mirrors.tuna.tsinghua.edu.cn/anaconda/cloud/conda-forge/linux-64/micromamba-1.5.5-0.tar.bz2
    tar -jxvf micromamba-1.5.5-0.tar.bz2
    #初始化micromamba到所有shell
    ./bin/micromamba shell init --shell bash --root-prefix=~/micromamba
    source ~/.bashrc
    #用micromamba配置环境
    #conda-forge 69MB, gcc 97MB, cmake 23MB, 不过不用担心, 已经安装过的会缓存, 可以重复使用.
    # 分步安装，确保ncurses=6.5优先级最高
    micromamba create -n terminal_game -c defaults  ncurses=6.5=h7934f7d_0 -y 
    micromamba activate terminal_game
    # 下面两行命令是我在docker中测试时用的, 因为docker中没有gcc等.
    # micromamba install -c conda-forge gxx>=11.3.0 cmake>=3.10.0 -y
    # micromamba install -c conda-forge git make -y
fi
cd -
