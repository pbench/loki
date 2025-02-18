#!/bin/bash
set -e

input="/storage/data"
output="/storage/mc_navitia"

rm -rf ${output}
mkdir -p ${output}
chmod -R 777 ${output}

#we want to be able to interupt the build, see: http://veithen.github.io/2014/11/16/sigterm-propagation.html
function run() {
    trap 'kill -TERM $PID' TERM INT
    $@ &
    PID=$!
    wait $PID
    trap - TERM INT
    wait $PID
    return $?
}


# we initialize the docker-compose.yml with the
# services used for all coverages
echo """
version: \"3\"
services:
  jormungandr:
    image: navitia/mc_jormun:latest
    volumes:
      - .:/data
    ports:
      - 9191:80

""" > ${output}/docker-compose.yml


# we initialize the kubernetes.yml with the
# services used for all coverages
echo """
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: volume-claim-navitia
spec:
  storageClassName: storage-class-navitia
  accessModes:
    - ReadOnlyMany
  resources:
    requests:
      storage: 100Mi
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: deployment-jormun
spec:
  replicas: 1
  selector:
    matchLabels:
      app: app-jormun
  template:
    metadata:
      labels:
        app : app-jormun
    spec:
      containers:
        - image: navitia/mc_jormun:latest
          name: jormungandr
          ports:
            - containerPort: 80
          volumeMounts:
            - name: volume-navitia
              mountPath: /data
      volumes:
        - name: volume-navitia
          persistentVolumeClaim:
            claimName: volume-claim-navitia
---
apiVersion: v1
kind: Service
metadata:
  name: navitia
spec:
  ports:
  - port: 80
    protocol: TCP
    targetPort: 80
  selector:
    app: app-jormun
  type: ClusterIP
""" > ${output}/kubernetes.yml

mkdir -p ${output}/jormun_conf/

cd ${input}
for folder in $(ls -d */); do
    coverage=${folder%%/}
    echo "Configuring ${coverage}"

    if [[ $coverage =~ "_" ]]; then
      echo "I can't handle a coverage name containing a '_' "
      echo "I'll skip coverage ${coverage}"
      continue
    fi

    if [[ ! -e ${input}/${coverage}/gtfs/ ]] && [[ ! -e ${input}/${coverage}/ntfs/ ]]; then
      echo "No gtfs/ nor ntfs/ subdirectory found in ${input}/${coverage}."
      echo "I skip coverage ${coverage}."
      continue
    fi

    if [[ -e ${input}/${coverage}/gtfs/ ]] && [[ -e ${input}/${coverage}/ntfs/ ]]; then
      echo "Found both gtfs/ nor ntfs/ subdirectory in ${input}/${coverage}."
      echo "I don't know which one to use so I skip the coverage ${coverage}."
      continue
    fi


    mkdir -p ${output}/${coverage}

    # copy gtfs data to output if present
    if [[ -e ${input}/${coverage}/gtfs/ ]]; then

      inputType="gtfs"

      rm -f ${output}/${coverage}/gtfs/*
      mkdir -p ${output}/${coverage}/gtfs/
      cp  ${input}/${coverage}/gtfs/* ${output}/${coverage}/gtfs/

      # remove "StopPoint:" prefix on stop point uris'
      sed -i 's/StopPoint://g' ${output}/${coverage}/gtfs/stops.txt
      sed -i 's/StopPoint://g' ${output}/${coverage}/gtfs/stop_times.txt
      if [[ -e ${input}/${coverage}/gtfs/transfers.txt ]]; then
        sed -i 's/StopPoint://g' ${output}/${coverage}/gtfs/transfers.txt
      fi
    fi

    # copy ntfs data to output if present
    if [[ -e ${input}/${coverage}/ntfs/ ]]; then
      inputType="ntfs"
      rm -f ${output}/${coverage}/ntfs/*
      mkdir -p ${output}/${coverage}/ntfs/
      cp  ${input}/${coverage}/ntfs/* ${output}/${coverage}/ntfs/
    fi

    # copy osm data to output if present
    if [[ -e ${input}/${coverage}/osm/ ]]; then
        rm -f ${output}/${coverage}/osm/*
        mkdir -p ${output}/${coverage}/osm/
        cp  ${input}/${coverage}/osm/* ${output}/${coverage}/osm/
    fi

    # copy geopal data to output if present
    if [[ -e ${input}/${coverage}/geopal/ ]]; then
        rm -f ${output}/${coverage}/geopal/*
        mkdir -p ${output}/${coverage}/geopal/
        zip -j -r ${output}/${coverage}/geopal/geopal.zip ${input}/${coverage}/geopal/
    fi

    # copy fusio-geopal data to output if present
    if [[ -e ${input}/${coverage}/fusio-geopal/ ]]; then
        rm -f ${output}/${coverage}/geopal/*
        mkdir -p ${output}/${coverage}/geopal/
        zip -j -r ${output}/${coverage}/geopal/geopal.zip ${input}/${coverage}/fusio-geopal/
    fi

    # binarize
    echo "Launch binarisation"
    rm -f ${output}/${coverage}/data.nav.lz4
    run python /navitia/source/eitri/eitri.py -d ${output}/${coverage}/ -e /usr/bin -o ${output}/${coverage}/data.nav.lz4

    # copy stoptimes_occupancy if present
    if [[ -e ${input}/${coverage}/stoptimes_occupancy.csv ]]; then
      cp ${input}/${coverage}/stoptimes_occupancy.csv ${output}/${coverage}/stoptimes_occupancy.csv
    fi



    # add kraken and loki services to docker for this coverage
    echo """
  loki-${coverage}:
    image: navitia/loki:dev
    environment:
      - RUST_LOG=debug
    volumes:
      - ./${coverage}/:/data

  kraken-${coverage}:
    image: navitia/mc_kraken:latest
    volumes:
      - ./${coverage}:/data
""" >> ${output}/docker-compose.yml



    krakenPort="30000"
    lokiPort="30001"

    # Jormun config files
    jq -n --arg instance "${coverage}" --arg krakenSocket "tcp://kraken-${coverage}:${krakenPort}" --arg lokiSocket "tcp://loki-${coverage}:${lokiPort}" '{
    key: $instance,
    zmq_socket: $krakenSocket,
    pt_planners: {
        loki: {
          klass: "jormungandr.pt_planners.loki.Loki",
          args: {
            timeout: 10000,
            zmq_socket: $lokiSocket
          }
        }
    }
}'  > ${output}/jormun_conf/${coverage}.json




    # kraken config file
    echo "[GENERAL]
instance_name = ${coverage}
database = /data/data.nav.lz4
zmq_socket = tcp://*:${krakenPort}

" > ${output}/${coverage}/kraken.ini

    # Loki config files
    echo """
    instance_name = '${coverage}'
    requests_socket = 'tcp://*:${lokiPort}'
    input_data_type = '${inputType}'
    [data_source]
    type = 'local'
    input_data_path = '/data/${inputType}/'
    occupancy_data_path = '/data/stoptimes_occupancy.csv'
    [rabbitmq]
    connect_retry_interval = '01:00:00'
""" > ${output}/${coverage}/loki_config.toml


  # add kraken and loki services to kubernetes for this coverage
    # kraken config file
    echo """
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: deployment-kraken-${coverage}
spec:
  replicas: 1
  selector:
    matchLabels:
      app: app-kraken-${coverage}
  template:
    metadata:
      labels:
        app : app-kraken-${coverage}
    spec:
      containers:
        - image: navitia/mc_kraken:latest
          name: kraken-${coverage}
          volumeMounts:
            - name: volume-navitia
              mountPath: /data
              subPath: ${coverage}
          ports:
            - containerPort: ${krakenPort}
      volumes:
        - name: volume-navitia
          persistentVolumeClaim:
            claimName: volume-claim-navitia
---
apiVersion: v1
kind: Service
metadata:
  name: kraken-${coverage}
spec:
  ports:
  - port: ${krakenPort}
    protocol: TCP
    targetPort: ${krakenPort}
  selector:
    app: app-kraken-${coverage}
  type: ClusterIP
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: deployment-loki-${coverage}
spec:
  replicas: 1
  selector:
    matchLabels:
      app: app-loki-${coverage}
  template:
    metadata:
      labels:
        app : app-loki-${coverage}
    spec:
      containers:
        - image: navitia/loki:dev
          name: loki-${coverage}
          volumeMounts:
            - name: volume-navitia
              mountPath: /data
              subPath: ${coverage}
          ports:
            - containerPort: ${lokiBasicPort}
            - containerPort: ${lokiLoadsPort}
      volumes:
        - name: volume-navitia
          persistentVolumeClaim:
            claimName: volume-claim-navitia
---
apiVersion: v1
kind: Service
metadata:
  name: loki-${coverage}
spec:
  ports:
  - port: ${lokiBasicPort}
    protocol: TCP
    targetPort: ${lokiBasicPort}
    name: basic
  - port: ${lokiLoadsPort}
    protocol: TCP
    targetPort: ${lokiLoadsPort}
    name: loads
  selector:
    app: app-loki-${coverage}
  type: ClusterIP
""" >> ${output}/kubernetes.yml

    echo "${coverage} done"
done


chmod -R 777 ${output}
